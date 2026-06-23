//! `OpenAPI` document generation (feature `openapi`).
//!
//! Emits a `pub fn openapi() -> utoipa::openapi::OpenApi` constructor for routers
//! invoked with the `openapi` flag. Like the rest of the crate this stays purely
//! *syntactic*: it reads the verb handlers' parameter/return tokens and recognises
//! a small set of axum wrappers (`Json`, `Form`, `Query`, `Path`) by their final
//! path segment — exactly as [`crate::parse::is_state_type`] does for `State`. It
//! never inspects a user type's fields; the schemas themselves are produced by the
//! compiler through `utoipa::ToSchema` / `utoipa::IntoParams` bounds evaluated at
//! the `#[folder_router]` invocation site.
//!
//! Because of that, every schema/param type written in a handler signature must be
//! *nameable at the invocation site* (import it there or write it fully qualified),
//! the same constraint that already applies to `intercept.rs` extractors.

use std::path::{Path, PathBuf};

use proc_macro2::TokenStream;
use quote::quote;

use crate::parse::{self, FolderRouterArgs, FolderRouterItem, FolderRouterRoutes, OperationMeta};

// URL segment for a single directory name: `[id]` -> `{id}`, `[...rest]` ->
// `{*rest}`, else the name as-is. (Mirrors `generate::dir_url_segment`.)
fn dir_url_segment(raw: &str) -> String {
    if raw.starts_with('[') && raw.ends_with(']') {
        let inner = &raw[1..raw.len() - 1];
        if let Some(stripped) = inner.strip_prefix("...") {
            format!("{{*{stripped}}}")
        } else {
            format!("{{{inner}}}")
        }
    } else {
        raw.to_string()
    }
}

// Directory components of a relative file path (the filename is dropped).
// `api/one/route.rs` → `["api", "one"]`; root `route.rs` → `[]`.
fn dir_components(rel_path: &Path) -> Vec<String> {
    let mut parts: Vec<String> = rel_path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    parts.pop(); // drop filename
    parts
}

// The absolute URL path for a `route.rs`, derived from its directory components
// (the trailing `route.rs` is dropped). A root-level `route.rs` yields "/".
//
// OpenAPI wants the full path from the API root regardless of how the router
// nests boundaries internally; since nesting prefix + relative path always
// reconstructs the absolute path, deriving it straight from the rel path is
// equivalent and simpler than replaying the nest logic.
fn route_url(rel_path: &Path) -> String {
    let url: Vec<String> = dir_components(rel_path)
        .iter()
        .map(|s| dir_url_segment(s))
        .collect();
    if url.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", url.join("/"))
    }
}

// Path-parameter names declared in a URL: both `{id}` and catch-all `{*rest}`.
fn path_param_names(url: &str) -> Vec<String> {
    url.split('/')
        .filter_map(|seg| {
            let inner = seg.strip_prefix('{')?.strip_suffix('}')?;
            Some(inner.strip_prefix('*').unwrap_or(inner).to_string())
        })
        .collect()
}

// Identifier of a type's final path segment, peeling references (so `&Json<T>`
// still reports `Json`). `None` for non-path types.
fn last_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(tp) => tp.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Reference(r) => last_ident(&r.elem),
        _ => None,
    }
}

// The first generic *type* argument of a path type, i.e. `T` in `Wrapper<T>`.
fn first_generic_type(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(tp) = ty else { return None };
    let seg = tp.path.segments.last()?;
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    args.args.iter().find_map(|a| match a {
        syn::GenericArgument::Type(t) => Some(t),
        _ => None,
    })
}

// A recognised request body: its content-type and payload type `T`.
struct BodyType<'a> {
    content_type: &'static str,
    ty: &'a syn::Type,
}

// First handler parameter that is a recognised request body (`Json<T>` /
// `Form<T>`), if any.
fn request_body(params: &[syn::Type]) -> Option<BodyType<'_>> {
    params.iter().find_map(|ty| match last_ident(ty).as_deref() {
        Some("Json") => first_generic_type(ty).map(|t| BodyType {
            content_type: "application/json",
            ty: t,
        }),
        Some("Form") => first_generic_type(ty).map(|t| BodyType {
            content_type: "application/x-www-form-urlencoded",
            ty: t,
        }),
        _ => None,
    })
}

// The `T` of the first `Query<T>` handler parameter, expanded into query
// parameters via `IntoParams`.
fn query_type(params: &[syn::Type]) -> Option<&syn::Type> {
    params.iter().find_map(|ty| {
        (last_ident(ty).as_deref() == Some("Query"))
            .then(|| first_generic_type(ty))
            .flatten()
    })
}

// The response body type `T`, recovered from a concrete return type: `Json<T>`,
// `Result<Json<T>, _>`, or a tuple containing a `Json<T>` (e.g. `(StatusCode,
// Json<T>)`). Returns `None` for opaque returns like `impl IntoResponse` — there
// is nothing to recover, so the operation gets a bodyless `200`.
fn response_type(ret: &syn::Type) -> Option<&syn::Type> {
    match last_ident(ret).as_deref() {
        Some("Json") => first_generic_type(ret),
        Some("Result") => first_generic_type(ret).and_then(response_type),
        _ => {
            if let syn::Type::Tuple(tuple) = ret {
                tuple.elems.iter().find_map(response_type)
            } else {
                None
            }
        }
    }
}

// `utoipa` tokens for a plain `string` schema (used for path parameters, whose
// concrete extractor type the macro doesn't try to introspect).
fn string_schema_tokens() -> TokenStream {
    quote! {
        utoipa::openapi::schema::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::SchemaType::Type(
                utoipa::openapi::schema::Type::String,
            ))
            .build()
    }
}

// The inner `T` of a `Vec<T>`, matched by final path segment (so `Vec<T>` and
// `std::vec::Vec<T>` both count). `None` for any other type.
fn vec_inner(ty: &syn::Type) -> Option<&syn::Type> {
    (last_ident(ty).as_deref() == Some("Vec"))
        .then(|| first_generic_type(ty))
        .flatten()
}

// The content schema for a body/response type: a `Ref` to the type's named
// component, except a `Vec<T>` (recursively) becomes an inline `array` whose
// `items` is the schema of `T`. A bare `Ref` to `Vec<T>` would otherwise name the
// component "Vec" (utoipa's `<Vec<_>>::name()`) and bury `T`'s own schema inside
// it. Both `Ref` and `ArrayBuilder` convert into `RefOr<Schema>` (the content
// schema) *and* `ArrayItems` (nested array items), so these same tokens work at
// the top level and recursively as an array's items.
fn content_schema_tokens(ty: &syn::Type) -> TokenStream {
    if let Some(inner) = vec_inner(ty) {
        let items = content_schema_tokens(inner);
        quote! {
            utoipa::openapi::schema::ArrayBuilder::new().items(#items).build()
        }
    } else {
        quote! {
            utoipa::openapi::schema::Ref::from_schema_name(
                <#ty as utoipa::ToSchema>::name()
            )
        }
    }
}

// Accumulate the component schemas for a body/response type into the runtime
// `__schemas` vec. A `Vec<T>` contributes `T`'s components (recursively) rather
// than a spurious "Vec" entry; any other type contributes itself plus its
// transitive `ToSchema` dependencies.
fn collect_schema_tokens(ty: &syn::Type) -> TokenStream {
    if let Some(inner) = vec_inner(ty) {
        return collect_schema_tokens(inner);
    }
    quote! {
        __schemas.push((
            <#ty as utoipa::ToSchema>::name().into(),
            <#ty as utoipa::PartialSchema>::schema(),
        ));
        <#ty as utoipa::ToSchema>::schemas(&mut __schemas);
    }
}

// The `utoipa` map of an axum verb to a `HttpMethod`. `any`/`connect` have no
// OpenAPI representation and are skipped (`None`).
fn http_method_tokens(method: &str) -> Option<TokenStream> {
    let variant = match method {
        "get" => "Get",
        "post" => "Post",
        "put" => "Put",
        "delete" => "Delete",
        "patch" => "Patch",
        "head" => "Head",
        "options" => "Options",
        "trace" => "Trace",
        // `any` and `connect` aren't expressible as a single OpenAPI operation.
        _ => return None,
    };
    let ident = syn::Ident::new(variant, proc_macro2::Span::call_site());
    Some(quote! { utoipa::openapi::path::HttpMethod::#ident })
}

// Split a doc string into (summary, description): the first line is the summary,
// the remaining lines (if any) the description.
fn split_doc(doc: &str) -> (String, Option<String>) {
    let mut lines = doc.splitn(2, '\n');
    let summary = lines.next().unwrap_or("").trim().to_string();
    let description = lines.next().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    (summary, description)
}

// Build an `Operation` expression for one verb handler, pushing any referenced
// component schemas into `__schemas` along the way.
// Derive a camelCase `operationId` from an HTTP verb and URL template, e.g.
// (`get`, `/api/me`) -> `getMe`, (`delete`, `/api/admin/api_keys/{id}`) ->
// `deleteAdminApiKeysById`. A single leading `api` segment is dropped as noise;
// path params become a trailing `By<Param>` (joined with `And` when several).
// The verb prefix keeps ids unique when one URL has multiple verbs.
fn operation_id(method: &str, url: &str) -> String {
    let mut segments: Vec<&str> = url.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() > 1 && segments.first() == Some(&"api") {
        segments.remove(0);
    }

    let mut name = method.to_string();
    let mut params: Vec<String> = Vec::new();
    for seg in segments {
        if let Some(param) = seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            params.push(pascal_case(param));
        } else {
            name.push_str(&pascal_case(seg));
        }
    }
    if !params.is_empty() {
        name.push_str("By");
        name.push_str(&params.join("And"));
    }
    name
}

// PascalCase a single path segment, splitting on `_`/`-` (e.g. `api_keys` ->
// `ApiKeys`).
fn pascal_case(segment: &str) -> String {
    segment
        .split(['_', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn operation_tokens(op: &OperationMeta, url: &str, tag: Option<&str>) -> TokenStream {
    let mut builder = quote! { utoipa::openapi::path::OperationBuilder::new() };

    // A stable `operationId` derived from the verb + URL. Client generators (e.g.
    // `@hey-api/openapi-ts`) name their generated functions from this, so emitting
    // it yields predictable SDK names instead of tool-derived fallbacks.
    let op_id = operation_id(op.method, url);
    builder = quote! { #builder.operation_id(Some(#op_id)) };

    if let Some(t) = tag {
        builder = quote! { #builder.tag(#t) };
    }

    if let Some(doc) = &op.doc {
        let (summary, description) = split_doc(doc);
        if !summary.is_empty() {
            builder = quote! { #builder.summary(Some(#summary)) };
        }
        if let Some(description) = description {
            builder = quote! { #builder.description(Some(#description)) };
        }
    }

    // Request body (`Json<T>` / `Form<T>`).
    if let Some(body) = request_body(&op.params) {
        let ct = body.content_type;
        let schema_ref = content_schema_tokens(body.ty);
        let collect = collect_schema_tokens(body.ty);
        builder = quote! {
            {
                #collect
                #builder.request_body(Some(
                    utoipa::openapi::request_body::RequestBodyBuilder::new()
                        .content(
                            #ct,
                            utoipa::openapi::ContentBuilder::new()
                                .schema(Some(#schema_ref))
                                .build(),
                        )
                        .build(),
                ))
            }
        };
    }

    // Responses: a typed `200` when the return body is recoverable, else a
    // bodyless `200`.
    let response = if let Some(ret) = &op.ret {
        if let Some(ty) = response_type(ret) {
            let schema_ref = content_schema_tokens(ty);
            let collect = collect_schema_tokens(ty);
            quote! {
                {
                    #collect
                    utoipa::openapi::ResponseBuilder::new()
                        .description("")
                        .content(
                            "application/json",
                            utoipa::openapi::ContentBuilder::new()
                                .schema(Some(#schema_ref))
                                .build(),
                        )
                        .build()
                }
            }
        } else {
            quote! { utoipa::openapi::ResponseBuilder::new().description("").build() }
        }
    } else {
        quote! { utoipa::openapi::ResponseBuilder::new().description("").build() }
    };
    builder = quote! {
        #builder.responses(
            utoipa::openapi::ResponsesBuilder::new()
                .response("200", #response)
                .build()
        )
    };

    // Parameters: path params (as `string`) followed by any `Query<T>` fields.
    let param_names = path_param_names(url);
    let path_pushes: Vec<TokenStream> = param_names
        .iter()
        .map(|name| {
            let schema = string_schema_tokens();
            quote! {
                __params.push(
                    utoipa::openapi::path::ParameterBuilder::new()
                        .name(#name)
                        .parameter_in(utoipa::openapi::path::ParameterIn::Path)
                        .required(utoipa::openapi::Required::True)
                        .schema(Some(#schema))
                        .build(),
                );
            }
        })
        .collect();
    let query_extend = query_type(&op.params).map(|ty| {
        quote! {
            __params.extend(
                <#ty as utoipa::IntoParams>::into_params(
                    || Some(utoipa::openapi::path::ParameterIn::Query),
                ),
            );
        }
    });
    let has_params = !path_pushes.is_empty() || query_extend.is_some();

    quote! {
        {
            let mut __params: Vec<utoipa::openapi::path::Parameter> = Vec::new();
            #( #path_pushes )*
            #query_extend
            let mut __op = #builder;
            if #has_params {
                __op = __op.parameters(Some(__params));
            }
            __op.build()
        }
    }
}

// Build the `PathItem` for one `route.rs` (all its verbs at one URL).
fn path_item_tokens(ops: &[OperationMeta], url: &str, tag: Option<&str>) -> Option<TokenStream> {
    // Pair each verb with its `HttpMethod`, dropping the unrepresentable ones.
    let methods: Vec<(TokenStream, TokenStream)> = ops
        .iter()
        .filter_map(|op| {
            http_method_tokens(op.method).map(|m| (m, operation_tokens(op, url, tag)))
        })
        .collect();

    if methods.is_empty() {
        return None;
    }
    // `.operation(method, op)` is a `PathItemBuilder` method (not `PathItem`), and
    // a `PathItem` holds at most one operation per verb — so chain every verb onto
    // the builder rather than seeding `PathItem::new` with the first. (Seeding
    // `PathItem::new(..).operation(..)` only compiles for single-verb paths, which
    // is all the fixtures exercised.)
    let op_calls = methods.iter().map(|(m, op)| quote! { .operation(#m, #op) });

    Some(quote! {
        utoipa::openapi::path::PathItemBuilder::new()
            #( #op_calls )*
            .build()
    })
}

// Per-directory configuration read from `openapi.toml`.
struct OpenApiConfig {
    include: Option<bool>,
    tag: Option<String>,
    auto_tag: Option<bool>,
}

// Parse an `openapi.toml` file. Returns the config on success or an error
// message to be surfaced as a `compile_error!`.
fn read_config(path: &Path) -> Result<OpenApiConfig, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let table: toml::Table = content
        .parse()
        .map_err(|e| format!("invalid TOML in {}: {e}", path.display()))?;
    Ok(OpenApiConfig {
        include: table.get("include").and_then(toml::Value::as_bool),
        tag: table.get("tag").and_then(toml::Value::as_str).map(str::to_string),
        auto_tag: table.get("auto_tag").and_then(toml::Value::as_bool),
    })
}

// Load all `openapi.toml` files into `(dir_components, OpenApiConfig)` pairs.
// Returns the config list on success; on error, extends `errors` with
// `compile_error!` tokens and returns `None`.
fn load_configs(
    config_files: &[(PathBuf, PathBuf)],
    errors: &mut TokenStream,
) -> Option<Vec<(Vec<String>, OpenApiConfig)>> {
    let mut configs = Vec::new();
    let mut had_error = false;
    for (abs, rel) in config_files {
        match read_config(abs) {
            Ok(cfg) => configs.push((dir_components(rel), cfg)),
            Err(msg) => {
                errors.extend(quote! { compile_error!(#msg); });
                had_error = true;
            }
        }
    }
    if had_error { None } else { Some(configs) }
}

// Walk ancestors (most-specific first) looking for the first config that sets
// `include`. Default is `false` (opt-in).
fn resolve_include(dir: &[String], configs: &[(Vec<String>, OpenApiConfig)]) -> bool {
    for prefix_len in (0..=dir.len()).rev() {
        let prefix = &dir[..prefix_len];
        if let Some((_, cfg)) = configs.iter().find(|(cd, _)| cd.as_slice() == prefix) {
            if let Some(inc) = cfg.include {
                return inc;
            }
        }
    }
    false
}

// Walk ancestors (most-specific first) looking for the first config that sets
// `tag` or `auto_tag`. Returns the resolved tag string, if any.
fn resolve_tag(dir: &[String], configs: &[(Vec<String>, OpenApiConfig)]) -> Option<String> {
    for prefix_len in (0..=dir.len()).rev() {
        let prefix = &dir[..prefix_len];
        if let Some((c_dir, cfg)) = configs.iter().find(|(cd, _)| cd.as_slice() == prefix) {
            if cfg.auto_tag == Some(true) || cfg.tag.is_some() {
                if cfg.auto_tag == Some(true) {
                    let c_len = c_dir.len();
                    if c_len < dir.len() {
                        // Use the segment immediately below c_dir toward dir.
                        return Some(dir[c_len].clone());
                    }
                    // Route sits directly in c_dir — fall back to explicit tag.
                    return cfg.tag.clone();
                }
                return cfg.tag.clone();
            }
        }
    }
    None
}

/// Emit `impl <Struct> { pub fn openapi() -> utoipa::openapi::OpenApi { … } }`,
/// or an empty stream when the `openapi` flag wasn't set. Independent of router
/// state, so it is always available regardless of the `into_router` /
/// `into_router_with_state` split.
pub fn openapi_impl(
    args: &FolderRouterArgs,
    item: &FolderRouterItem,
    routes: &FolderRouterRoutes,
) -> TokenStream {
    if !args.openapi {
        return TokenStream::new();
    }

    let struct_name = item.struct_name();

    let mut errors = TokenStream::new();
    let Some(configs) = load_configs(routes.config_files(), &mut errors) else {
        return errors;
    };

    let mut path_entries: Vec<TokenStream> = Vec::new();
    for (route_path, rel_path) in routes.routes() {
        let dir = dir_components(rel_path);
        if !resolve_include(&dir, &configs) {
            continue;
        }
        let tag = resolve_tag(&dir, &configs);
        let url = route_url(rel_path);
        let ops = parse::operations_for_route(route_path);
        if let Some(path_item) = path_item_tokens(&ops, &url, tag.as_deref()) {
            path_entries.push(quote! { .path(#url, #path_item) });
        }
    }

    quote! {
        impl #struct_name {
            /// The OpenAPI document for this router, generated from the route tree.
            ///
            /// Schemas are produced via `utoipa::ToSchema` / `utoipa::IntoParams`
            /// on the handler types. Merge or augment the returned document with
            /// the `utoipa::openapi::OpenApiBuilder` API (title, version, servers,
            /// security, …) before serving it.
            #[allow(clippy::too_many_lines)]
            pub fn openapi() -> utoipa::openapi::OpenApi {
                let mut __schemas: Vec<(
                    String,
                    utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
                )> = Vec::new();

                let __paths = utoipa::openapi::path::PathsBuilder::new()
                    #( #path_entries )*
                    .build();

                let mut __components = utoipa::openapi::schema::ComponentsBuilder::new();
                for (__name, __schema) in __schemas {
                    __components = __components.schema(__name, __schema);
                }

                utoipa::openapi::OpenApiBuilder::new()
                    .paths(__paths)
                    .components(Some(__components.build()))
                    .build()
            }
        }
    }
}
