use std::{
    fs,
    path::{Path, PathBuf},
};

use quote::ToTokens;
use syn::{
    parse::{Parse, ParseStream},
    parse_file,
    Item,
    LitStr,
    Result,
    Token,
    Visibility,
};

pub struct FolderRouterArgs {
    pub path: String,
    /// The app state type the generated router is parameterized over. Parsed as
    /// a full `syn::Type` (not just an `Ident`) so callers can pass a reference
    /// type like `&'static AppState` and have the per-request `State` clone be a
    /// pointer copy rather than a deep clone.
    pub state_type: syn::Type,
    /// Whether the invocation requested OpenAPI generation via a trailing
    /// `, openapi` flag (e.g. `#[folder_router("api", AppState, openapi)]`). When
    /// set — and the crate's `openapi` feature is enabled — the macro emits an
    /// `openapi()` constructor alongside `into_router*`.
    pub openapi: bool,
}

impl FolderRouterArgs {
    /// The route directory as an absolute path, resolved against the Cargo
    /// manifest directory.
    pub fn abs_path(&self) -> PathBuf {
        let manifest_dir = Self::get_manifest_dir();
        Path::new(&manifest_dir).join(&self.path)
    }

    // This is a workaround for macrotest behaviour
    #[cfg(feature = "macrotest")]
    fn get_manifest_dir() -> String {
        use regex::Regex;
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or("./".to_string());
        // Match both `/` and `\` separators so this works on Windows too.
        let re = Regex::new(r"^(.+)[/\\]target[/\\]tests[/\\]axum-folder-router[/\\][A-Za-z0-9]{42}$")
            .unwrap();

        if let Some(captures) = re.captures(&dir) {
            captures.get(1).unwrap().as_str().to_string()
        } else {
            dir
        }
    }

    #[cfg(not(feature = "macrotest"))]
    fn get_manifest_dir() -> String {
        std::env::var("CARGO_MANIFEST_DIR").unwrap_or("./".to_string())
    }
}
impl Parse for FolderRouterArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        let path_lit = input.parse::<LitStr>()?;
        input.parse::<Token![,]>()?;
        let state_type = input.parse::<syn::Type>()?;

        // Optional trailing flags, comma-separated. Currently only `openapi`.
        let mut openapi = false;
        while input.parse::<Token![,]>().is_ok() {
            if input.is_empty() {
                break; // tolerate a trailing comma
            }
            let flag = input.parse::<syn::Ident>()?;
            if flag == "openapi" {
                openapi = true;
            } else {
                return Err(syn::Error::new(
                    flag.span(),
                    format!("unknown `folder_router` flag `{flag}`; expected `openapi`"),
                ));
            }
        }

        Ok(FolderRouterArgs {
            path: path_lit.value(),
            state_type,
            openapi,
        })
    }
}

/// Parses the file at the specified location and returns HTTP verb functions
pub fn methods_for_route(route_path: &PathBuf) -> Vec<&'static str> {
    // Read the file content
    let Ok(file_content) = fs::read_to_string(route_path) else {
        return Vec::new();
    };

    // Parse the file content into a syn syntax tree
    let Ok(file) = parse_file(&file_content) else {
        return Vec::new();
    };

    // Define HTTP methods we're looking for
    let allowed_methods = [
        "any", "get", "post", "put", "delete", "patch", "head", "options", "trace", "connect",
    ];
    let mut found_methods = Vec::new();

    // Collect all pub & async fn's
    for item in &file.items {
        if let Item::Fn(fn_item) = item {
            let fn_name = fn_item.sig.ident.to_string();
            let is_public = matches!(fn_item.vis, Visibility::Public(_));
            let is_async = fn_item.sig.asyncness.is_some();

            if is_public && is_async {
                found_methods.push(fn_name);
            }
        }
    }

    // Iterate through methods to ensure consistent order
    allowed_methods
        .into_iter()
        .filter(|elem| found_methods.iter().any(|method| method.as_str() == *elem))
        .collect()
}

/// Metadata about a single HTTP-verb handler in a `route.rs`, collected for
/// `OpenAPI` generation. Like the rest of this crate it is purely *syntactic*: the
/// parameter and return types are the tokens as written, and the actual schemas
/// are recovered later by the compiler via `utoipa::ToSchema`/`IntoParams` bounds
/// at the generated call site.
#[cfg(feature = "openapi")]
#[derive(Clone)]
pub struct OperationMeta {
    /// The HTTP verb (handler fn name), e.g. `"get"`.
    pub method: &'static str,
    /// The handler's parameter types, in order (axum extractors).
    pub params: Vec<syn::Type>,
    /// The handler's return type, if written explicitly (`None` for `-> ()`).
    pub ret: Option<syn::Type>,
    /// The handler's doc comment (joined `#[doc]` lines, trimmed), if any. Its
    /// first line becomes the operation summary, the remainder the description.
    pub doc: Option<String>,
}

/// Join a function's `#[doc = "…"]` attributes into a single trimmed string,
/// returning `None` when there are none.
#[cfg(feature = "openapi")]
fn doc_string(attrs: &[syn::Attribute]) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
            {
                // rustdoc strips a single leading space from each `///` line.
                lines.push(s.value().strip_prefix(' ').map_or_else(|| s.value(), str::to_owned));
            }
        }
    }
    let joined = lines.join("\n");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Parse a `route.rs` into per-verb [`OperationMeta`], mirroring the verb
/// detection in [`methods_for_route`] but also capturing each handler's
/// parameter/return types and doc comment for `OpenAPI` emission. Returned in the
/// same canonical verb order as [`methods_for_route`].
#[cfg(feature = "openapi")]
pub fn operations_for_route(route_path: &PathBuf) -> Vec<OperationMeta> {
    let allowed_methods = [
        "any", "get", "post", "put", "delete", "patch", "head", "options", "trace", "connect",
    ];

    let Ok(file_content) = fs::read_to_string(route_path) else {
        return Vec::new();
    };
    let Ok(file) = parse_file(&file_content) else {
        return Vec::new();
    };

    let mut found: Vec<OperationMeta> = Vec::new();
    for item in &file.items {
        let Item::Fn(fn_item) = item else { continue };
        if !matches!(fn_item.vis, Visibility::Public(_)) || fn_item.sig.asyncness.is_none() {
            continue;
        }
        let Some(method) = allowed_methods
            .into_iter()
            .find(|m| fn_item.sig.ident == m)
        else {
            continue;
        };

        let params = fn_item
            .sig
            .inputs
            .iter()
            .filter_map(|arg| match arg {
                syn::FnArg::Typed(pt) => Some((*pt.ty).clone()),
                syn::FnArg::Receiver(_) => None,
            })
            .collect();
        let ret = match &fn_item.sig.output {
            syn::ReturnType::Type(_, ty) => Some((**ty).clone()),
            syn::ReturnType::Default => None,
        };

        found.push(OperationMeta {
            method,
            params,
            ret,
            doc: doc_string(&fn_item.attrs),
        });
    }

    // Canonical verb order, matching `methods_for_route`.
    let mut ordered = Vec::new();
    for verb in allowed_methods {
        if let Some(op) = found.iter().find(|op| op.method == verb) {
            ordered.push(op.clone());
        }
    }
    ordered
}

// Collect `route.rs`, `middleware.rs`, `fallback.rs`, `intercept.rs`, and
// (when the `openapi` feature is active) `openapi.toml` files recursively.
pub fn collect_files(
    base_dir: &Path,
    dir: &Path,
    routes: &mut Vec<(PathBuf, PathBuf)>,
    middleware: &mut Vec<(PathBuf, PathBuf)>,
    fallback: &mut Vec<(PathBuf, PathBuf)>,
    intercept: &mut Vec<(PathBuf, PathBuf)>,
    #[cfg(feature = "openapi")] config: &mut Vec<(PathBuf, PathBuf)>,
) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(std::result::Result::ok) {
            let path = entry.path();

            if path.is_dir() {
                collect_files(
                    base_dir,
                    &path,
                    routes,
                    middleware,
                    fallback,
                    intercept,
                    #[cfg(feature = "openapi")]
                    config,
                );
            } else if let Ok(rel_dir) = path.strip_prefix(base_dir) {
                match path.file_name().and_then(|n| n.to_str()) {
                    Some("route.rs") => routes.push((path.clone(), rel_dir.to_path_buf())),
                    Some("middleware.rs") => middleware.push((path.clone(), rel_dir.to_path_buf())),
                    Some("fallback.rs") => fallback.push((path.clone(), rel_dir.to_path_buf())),
                    Some("intercept.rs") => intercept.push((path.clone(), rel_dir.to_path_buf())),
                    #[cfg(feature = "openapi")]
                    Some("openapi.toml") => config.push((path.clone(), rel_dir.to_path_buf())),
                    _ => {}
                }
            }
        }
    }
}

/// How a router-transform file's function (`pub fn middleware` / `pub fn
/// fallback`) consumes router state. It's an arity classification, shared by
/// `middleware.rs` and `fallback.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiddlewareKind {
    /// `fn(router) -> router` — wired before state exists.
    Stateless,
    /// `fn(router, state) -> router` — receives a clone of the app state value,
    /// so it can build e.g. `from_fn_with_state` layers.
    Stateful,
}

/// Inspects the file at `path` for a `pub fn <fn_name>` and reports how it
/// consumes state, based on its arity: one parameter (`router`) is
/// [`MiddlewareKind::Stateless`], two or more (`router, state`) is
/// [`MiddlewareKind::Stateful`]. Returns `None` when no such function exists.
///
/// Only the name/visibility/arity are inspected, not the full signature; a
/// function with the wrong shape still surfaces as a regular compiler error at
/// the generated call site.
fn fn_kind(path: &Path, fn_name: &str) -> Option<MiddlewareKind> {
    let file_content = fs::read_to_string(path).ok()?;
    let file = parse_file(&file_content).ok()?;

    file.items.iter().find_map(|item| {
        let Item::Fn(fn_item) = item else {
            return None;
        };
        if !matches!(fn_item.vis, Visibility::Public(_)) || fn_item.sig.ident != fn_name {
            return None;
        }
        Some(if fn_item.sig.inputs.len() >= 2 {
            MiddlewareKind::Stateful
        } else {
            MiddlewareKind::Stateless
        })
    })
}

/// Kind of the `pub fn middleware` in a `middleware.rs`, or `None` if absent.
pub fn middleware_kind(path: &Path) -> Option<MiddlewareKind> {
    fn_kind(path, "middleware")
}

/// Kind of the `pub fn fallback` in a `fallback.rs`, or `None` if absent.
pub fn fallback_kind(path: &Path) -> Option<MiddlewareKind> {
    fn_kind(path, "fallback")
}

/// The parsed signature of a `pub async fn intercept`: its parameter types in
/// order (the last of which is the forwarded `Request`) and whether any of them
/// is a `State<…>` extractor. Unlike middleware/fallback — whose `(router[,
/// state])` shape is a pure arity classification — an intercept's parameters are
/// axum extractors, so the macro keeps the full type list to reproduce them on
/// the generated `from_fn` layer, and uses the presence of `State<…>` (rather
/// than arity) to decide whether the layer needs the app state.
#[derive(Clone)]
pub struct InterceptSig {
    pub params: Vec<syn::Type>,
    pub stateful: bool,
}

/// The identifier of a type's final path segment, peeling references so
/// `&State<_>` still reports `State`. `None` for non-path types.
fn type_last_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(tp) => tp.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Reference(r) => type_last_ident(&r.elem),
        _ => None,
    }
}

/// Whether a parameter type is an axum `State<…>` extractor, matched by the last
/// path segment so `State<_>` and `axum::extract::State<_>` both count.
pub fn is_state_type(ty: &syn::Type) -> bool {
    type_last_ident(ty).as_deref() == Some("State")
}

/// Whether a parameter type is the forwarded `Request`, matched by name so
/// `Request`, `axum::extract::Request` and `http::Request<_>` all count.
pub fn is_request_type(ty: &syn::Type) -> bool {
    type_last_ident(ty).as_deref() == Some("Request")
}

/// Parse the `pub async fn intercept` in an `intercept.rs` into an
/// [`InterceptSig`], or `None` if no such function exists. Only the
/// name/visibility and parameter *types* are inspected; the return type and
/// parameter patterns are left to the generated call site to type-check.
pub fn intercept_signature(path: &Path) -> Option<InterceptSig> {
    let file_content = fs::read_to_string(path).ok()?;
    let file = parse_file(&file_content).ok()?;

    file.items.iter().find_map(|item| {
        let Item::Fn(fn_item) = item else {
            return None;
        };
        if !matches!(fn_item.vis, Visibility::Public(_)) || fn_item.sig.ident != "intercept" {
            return None;
        }
        let params: Vec<syn::Type> = fn_item
            .sig
            .inputs
            .iter()
            .filter_map(|arg| match arg {
                syn::FnArg::Typed(pt) => Some((*pt.ty).clone()),
                syn::FnArg::Receiver(_) => None,
            })
            .collect();
        let stateful = params.iter().any(is_state_type);
        Some(InterceptSig { params, stateful })
    })
}

pub struct FolderRouterItem {
    item: syn::ItemStruct,
}

impl FolderRouterItem {
    pub fn module_namespace(&self) -> syn::Path {
        syn::parse_str(&format!(
            "__folder_router__{}",
            self.item
                .ident
                .to_string()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .map(|c| c.to_ascii_lowercase())
                .collect::<String>(),
        ))
        .unwrap()
    }

    pub fn struct_name(&self) -> syn::Ident {
        self.item.ident.clone()
    }
}

impl Parse for FolderRouterItem {
    fn parse(input: ParseStream) -> Result<Self> {
        let item: syn::ItemStruct = input.parse()?;

        Ok(Self {
            item,
        })
    }
}

impl ToTokens for FolderRouterItem {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        self.item.to_tokens(tokens);
    }
}

pub struct FolderRouterRoutes {
    routes: Vec<(PathBuf, PathBuf)>,
    middleware: Vec<(PathBuf, PathBuf, MiddlewareKind)>,
    fallback: Vec<(PathBuf, PathBuf, MiddlewareKind)>,
    intercept: Vec<(PathBuf, PathBuf, InterceptSig)>,
    #[cfg(feature = "openapi")]
    config: Vec<(PathBuf, PathBuf)>,
}

/// Validate the discovered `intercept.rs` files and pair each with its parsed
/// signature. An `intercept.rs` must define a `pub async fn intercept` whose
/// parameters are axum extractors ending in the forwarded `Request` (the macro
/// then applies it as a per-request `.layer` over the whole subtree, including
/// the fallback). Files that fail either check emit a `compile_error!` and are
/// dropped (so no broken layer is generated for them).
fn parse_intercepts(
    errors: &mut proc_macro2::TokenStream,
    raw_intercept: Vec<(PathBuf, PathBuf)>,
) -> Vec<(PathBuf, PathBuf, InterceptSig)> {
    let mut intercept = Vec::new();
    for (ic_path, rel) in raw_intercept {
        let Some(sig) = intercept_signature(&ic_path) else {
            let ic_str = ic_path.to_string_lossy().into_owned();
            errors.extend(quote::quote! {
                compile_error!(concat!(
                    "`intercept.rs` found but it does not define a `pub async fn intercept`: '",
                    #ic_str,
                    "'. Define `pub async fn intercept(/* extractors… */ req: axum::extract::Request) ",
                    "-> std::ops::ControlFlow<axum::response::Response, axum::extract::Request>`. ",
                    "Use a `State<S>` parameter to receive the app state."
                ));
            });
            continue;
        };

        // The forwarded `Request` must appear exactly once and be the final
        // parameter: axum requires the body-consuming extractor last, and the
        // macro appends `Next` after it. (Any other extractors come first.)
        let req_positions: Vec<usize> = sig
            .params
            .iter()
            .enumerate()
            .filter(|(_, ty)| is_request_type(ty))
            .map(|(i, _)| i)
            .collect();
        let req_ok = req_positions.len() == 1 && req_positions[0] == sig.params.len() - 1;
        if !req_ok {
            let ic_str = ic_path.to_string_lossy().into_owned();
            errors.extend(quote::quote! {
                compile_error!(concat!(
                    "`intercept.rs` must take exactly one `axum::extract::Request` parameter, ",
                    "and it must be the *last* parameter (extractors come first): '",
                    #ic_str,
                    "'. e.g. `pub async fn intercept(State(s): axum::extract::State<S>, ",
                    "req: axum::extract::Request) ",
                    "-> std::ops::ControlFlow<axum::response::Response, axum::extract::Request>`."
                ));
            });
            continue;
        }

        intercept.push((ic_path, rel, sig));
    }
    intercept
}

impl FolderRouterRoutes {
    pub fn parse_from_path(errors: &mut proc_macro2::TokenStream, path: &Path) -> Self {
        let mut routes = Vec::new();
        let mut raw_middleware = Vec::new();
        let mut raw_fallback = Vec::new();
        let mut raw_intercept = Vec::new();
        #[cfg(feature = "openapi")]
        let mut config = Vec::new();
        collect_files(
            path,
            path,
            &mut routes,
            &mut raw_middleware,
            &mut raw_fallback,
            &mut raw_intercept,
            #[cfg(feature = "openapi")]
            &mut config,
        );
        routes.sort();
        raw_middleware.sort();
        raw_fallback.sort();
        raw_intercept.sort();
        #[cfg(feature = "openapi")]
        config.sort();

        let path_cow = path.to_string_lossy();
        let path_str = path_cow.as_ref();

        if routes.is_empty() && raw_fallback.is_empty() {
            errors.extend(quote::quote! {
                compile_error!(concat!(
                    "No route.rs or fallback.rs files found in the specified directory: '",
                    #path_str,
                    "'. Make sure the path is correct and contains at least one route.rs ",
                    "(with an HTTP verb handler) or a fallback.rs (e.g. a ServeDir static server)."
                ));
            });
        }

        // Every `middleware.rs` must expose a `pub fn middleware`, otherwise the
        // generated subtree wiring would fail to compile with a confusing error.
        // Capture each one's arity (stateless vs stateful) while we're here.
        let mut middleware = Vec::new();
        for (mw_path, rel) in raw_middleware {
            if let Some(kind) = middleware_kind(&mw_path) {
                middleware.push((mw_path, rel, kind));
            } else {
                let mw_str = mw_path.to_string_lossy().into_owned();
                errors.extend(quote::quote! {
                    compile_error!(concat!(
                        "`middleware.rs` found but it does not define a `pub fn middleware`: '",
                        #mw_str,
                        "'. Define `pub fn middleware(router: axum::Router<S>) -> axum::Router<S>` ",
                        "or `pub fn middleware(router: axum::Router<S>, state: S) -> axum::Router<S>`."
                    ));
                });
            }
        }

        // A `fallback.rs` exposes a `pub fn fallback` and applies to its subtree
        // (its own dir plus all nested routes). Subtrees are composed with
        // `Router::nest`, so each can own its own fallback; the generated code
        // threads ancestor fallbacks down so a deeper one is never silently
        // overridden. (A `fallback.rs` in a catch-all dir is rejected at the
        // nest site, since axum forbids wildcards in a `nest` prefix.)
        let mut fallback = Vec::new();
        for (fb_path, rel) in raw_fallback {
            if let Some(kind) = fallback_kind(&fb_path) {
                fallback.push((fb_path, rel, kind));
            } else {
                let fb_str = fb_path.to_string_lossy().into_owned();
                errors.extend(quote::quote! {
                    compile_error!(concat!(
                        "`fallback.rs` found but it does not define a `pub fn fallback`: '",
                        #fb_str,
                        "'. Define `pub fn fallback(router: axum::Router<S>) -> axum::Router<S>` ",
                        "or `pub fn fallback(router: axum::Router<S>, state: S) -> axum::Router<S>`."
                    ));
                });
            }
        }

        let intercept = parse_intercepts(errors, raw_intercept);

        Self {
            routes,
            middleware,
            fallback,
            intercept,
            #[cfg(feature = "openapi")]
            config,
        }
    }

    /// The discovered `route.rs` files as `(absolute path, path relative to the
    /// base dir)` pairs, sorted for deterministic output.
    pub fn routes(&self) -> &[(PathBuf, PathBuf)] {
        &self.routes
    }

    /// The discovered `middleware.rs` files as `(absolute path, path relative to
    /// the base dir, kind)`, sorted for deterministic output.
    pub fn middleware(&self) -> &[(PathBuf, PathBuf, MiddlewareKind)] {
        &self.middleware
    }

    /// The discovered `fallback.rs` files as `(absolute path, path relative to
    /// the base dir, kind)`, sorted for deterministic output. One per owning
    /// directory, since each subtree can declare its own fallback.
    pub fn fallback(&self) -> &[(PathBuf, PathBuf, MiddlewareKind)] {
        &self.fallback
    }

    /// The discovered `intercept.rs` files as `(absolute path, path relative to
    /// the base dir, signature)`, sorted for deterministic output. One per owning
    /// directory; applied as a per-request layer over that subtree.
    pub fn intercept(&self) -> &[(PathBuf, PathBuf, InterceptSig)] {
        &self.intercept
    }

    /// The discovered `openapi.toml` config files as `(absolute path, path
    /// relative to the base dir)` pairs, sorted for deterministic output.
    #[cfg(feature = "openapi")]
    pub fn config_files(&self) -> &[(PathBuf, PathBuf)] {
        &self.config
    }
}
