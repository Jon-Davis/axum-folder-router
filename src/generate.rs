use std::{collections::BTreeMap, path::Path};

use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::LitStr;

use crate::parse::{self, is_request_type, is_state_type, methods_for_route, InterceptSig, MiddlewareKind};

// A struct representing a directory in the module tree
#[derive(Debug)]
struct ModuleDir {
    name: String,
    has_route: bool,
    has_middleware: bool,
    has_fallback: bool,
    has_intercept: bool,
    children: BTreeMap<String, ModuleDir>,
}

impl ModuleDir {
    fn new(name: &str) -> Self {
        ModuleDir {
            name: name.to_string(),
            has_route: false,
            has_middleware: false,
            has_fallback: false,
            has_intercept: false,
            children: BTreeMap::new(),
        }
    }

    // Register a `route.rs`/`middleware.rs`/`fallback.rs` file (by its path
    // relative to the base dir) into the module tree, creating intermediate dirs
    // as needed.
    fn add_to_module_tree(&mut self, rel_path: &Path) {
        let components: Vec<_> = rel_path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();

        let mut root = self;

        for (i, segment) in components.iter().enumerate() {
            if i == components.len() - 1 {
                match segment.as_str() {
                    "route.rs" => root.has_route = true,
                    "middleware.rs" => root.has_middleware = true,
                    "fallback.rs" => root.has_fallback = true,
                    "intercept.rs" => root.has_intercept = true,
                    _ => {}
                }
                break;
            }

            root = root
                .children
                .entry(segment.clone())
                .or_insert_with(|| ModuleDir::new(segment));
        }
    }
}

// Normalize a path segment for use as a module name
fn normalize_module_name(name: &str) -> String {
    if name.starts_with('[') && name.ends_with(']') {
        let inner = &name[1..name.len() - 1];
        if let Some(stripped) = inner.strip_prefix("...") {
            format!("___{stripped}")
        } else {
            format!("__{inner}")
        }
    } else {
        name.replace(['-', '.'], "_")
    }
}

// Convert a relative path to module path segments. The URL is derived
// separately at emit time (see `emit_into`/`url_from`).
fn path_to_module_path(rel_path: &Path) -> Vec<String> {
    let components: Vec<_> = rel_path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();

    // Handle root route
    if components.is_empty() {
        return vec!["route".to_string()];
    }

    let mut mod_path = Vec::new();
    for (i, segment) in components.iter().enumerate() {
        if i == components.len() - 1 && segment == "route.rs" {
            mod_path.push("route".to_string());
        } else {
            mod_path.push(normalize_module_name(segment));
        }
    }

    mod_path
}

// URL segment for a single directory name (mirrors the per-segment logic in
// `path_to_module_path`): `[id]` -> `{id}`, `[...rest]` -> `{*rest}`, else as-is.
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

// Join URL segments into an axum path (relative to the enclosing nest). Empty
// segments yield "/".
fn url_from(segments: &[String]) -> String {
    if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segments.join("/"))
    }
}

// Whether any segment is a catch-all (`{*...}`). axum panics if a `nest` prefix
// contains a wildcard, so a subtree reached through one can't own a
// `middleware.rs`/`fallback.rs`.
fn has_catchall(segments: &[String]) -> bool {
    segments.iter().any(|s| s.starts_with("{*"))
}

// Generate tokens for a module path
fn generate_mod_path_tokens(mod_path: &[String]) -> TokenStream {
    let mut result = TokenStream::new();

    for (i, segment) in mod_path.iter().enumerate() {
        let segment_ident = format_ident!("{}", segment);

        if i == 0 {
            result = quote! { #segment_ident };
        } else {
            result = quote! { #result::#segment_ident };
        }
    }

    result
}

// Generate module hierarchy code
fn generate_module_hierarchy(dir: &ModuleDir) -> TokenStream {
    let mut result = TokenStream::new();

    // Add route.rs module if this directory has one
    if dir.has_route {
        let route_mod = quote! {
            #[path = "route.rs"]
            pub mod route;
        };
        result.extend(route_mod);
    }

    // Add middleware.rs module if this directory has one
    if dir.has_middleware {
        let middleware_mod = quote! {
            #[path = "middleware.rs"]
            pub mod middleware;
        };
        result.extend(middleware_mod);
    }

    // Add fallback.rs module if this directory has one
    if dir.has_fallback {
        let fallback_mod = quote! {
            #[path = "fallback.rs"]
            pub mod fallback;
        };
        result.extend(fallback_mod);
    }

    // Add intercept.rs module if this directory has one
    if dir.has_intercept {
        let intercept_mod = quote! {
            #[path = "intercept.rs"]
            pub mod intercept;
        };
        result.extend(intercept_mod);
    }

    // Add subdirectories
    for child in dir.children.values() {
        let child_name = format_ident!("{}", normalize_module_name(&child.name));
        let child_path_lit = LitStr::new(&child.name, proc_macro2::Span::call_site());
        let child_content = generate_module_hierarchy(child);

        let child_mod = quote! {
            #[path = #child_path_lit]
            pub mod #child_name {
                #child_content
            }
        };

        result.extend(child_mod);
    }

    result
}

// A node in the route tree, mirroring the directory structure. Each node owns
// the routes/middleware/fallback defined directly in its directory plus its
// children, so a `middleware.rs`/`fallback.rs` applies to exactly its subtree.
#[derive(Default)]
struct RouteNode {
    children: BTreeMap<String, RouteNode>,
    // (module path segments, http methods) for this dir's `route.rs`. The URL is
    // computed at emit time from the path accumulated since the enclosing nest.
    route: Option<(Vec<String>, Vec<&'static str>)>,
    // `Some(kind)` if this dir has a `middleware.rs`, recording whether it takes
    // the app state value.
    middleware: Option<MiddlewareKind>,
    // `Some(kind)` if this dir has a `fallback.rs`, recording whether it takes
    // the app state value.
    fallback: Option<MiddlewareKind>,
    // `Some(sig)` if this dir has an `intercept.rs`, holding its extractor
    // parameter types (reproduced on the generated layer) and whether it takes
    // the app state (a `State<…>` param, so stateful means `from_fn_with_state`).
    intercept: Option<InterceptSig>,
    // normalized module path segments leading to this directory (used to
    // reference its `middleware`/`fallback`/`intercept` module).
    mod_segments: Vec<String>,
}

// Directory segments of a `route.rs`/`middleware.rs` rel path (i.e. without the
// trailing file name). A root-level file yields an empty Vec.
fn dir_components(rel_path: &Path) -> Vec<String> {
    let mut components: Vec<String> = rel_path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    components.pop();
    components
}

// Descend into (creating as needed) the node for the given directory segments.
fn node_at<'a>(root: &'a mut RouteNode, dir_components: &[String]) -> &'a mut RouteNode {
    let mut node = root;
    let mut acc = Vec::new();
    for segment in dir_components {
        acc.push(normalize_module_name(segment));
        let mod_segments = acc.clone();
        node = node.children.entry(segment.clone()).or_insert_with(|| RouteNode {
            mod_segments,
            ..RouteNode::default()
        });
    }
    node
}

fn build_route_tree(routes: &parse::FolderRouterRoutes) -> RouteNode {
    let mut root = RouteNode::default();

    for (route_path, rel_path) in routes.routes() {
        let mod_path = path_to_module_path(rel_path);
        let method_registrations = methods_for_route(route_path);

        #[cfg(feature = "debug")]
        println!(
            "/// [folder_router] Found route.rs for mod_path: {:?}, methods: {:?}",
            mod_path, method_registrations
        );

        let node = node_at(&mut root, &dir_components(rel_path));
        node.route = Some((mod_path, method_registrations));
    }

    for (_mw_path, rel_path, kind) in routes.middleware() {
        #[cfg(feature = "debug")]
        println!(
            "/// [folder_router] Found middleware.rs ({:?}) for dir: {:?}",
            kind,
            dir_components(rel_path)
        );

        let node = node_at(&mut root, &dir_components(rel_path));
        node.middleware = Some(*kind);
    }

    for (_fb_path, rel_path, kind) in routes.fallback() {
        #[cfg(feature = "debug")]
        println!(
            "/// [folder_router] Found fallback.rs ({:?}) for dir: {:?}",
            kind,
            dir_components(rel_path)
        );

        let node = node_at(&mut root, &dir_components(rel_path));
        node.fallback = Some(*kind);
    }

    for (_ic_path, rel_path, sig) in routes.intercept() {
        #[cfg(feature = "debug")]
        println!(
            "/// [folder_router] Found intercept.rs (stateful: {:?}) for dir: {:?}",
            sig.stateful,
            dir_components(rel_path)
        );

        let node = node_at(&mut root, &dir_components(rel_path));
        node.intercept = Some(sig.clone());
    }

    root
}

// Whether the tree registers at least one route method anywhere.
fn has_any_registration(node: &RouteNode) -> bool {
    node.route
        .as_ref()
        .is_some_and(|(_, methods)| !methods.is_empty())
        || node.children.values().any(has_any_registration)
}

// Build the `axum::routing::get(..).post(..)` method-router for a single route.
fn method_router(
    mod_namespace: &syn::Path,
    mod_path: &[String],
    methods: &[&'static str],
) -> TokenStream {
    let mod_path_tokens = generate_mod_path_tokens(mod_path);
    let first = format_ident!("{}", methods[0]);
    let mut builder = quote! {
        axum::routing::#first(#mod_namespace::#mod_path_tokens::#first)
    };
    for method in &methods[1..] {
        let method_ident = format_ident!("{}", method);
        builder = quote! {
            #builder.#method_ident(#mod_namespace::#mod_path_tokens::#method_ident)
        };
    }
    builder
}

// Whether this node or any descendant defines a `middleware.rs` or `fallback.rs`
// — i.e. a "boundary" that must be built as its own nested sub-router (so its
// middleware scopes to, and its fallback owns, exactly that subtree). A
// boundary-free subtree is registered flat onto the ambient router instead.
fn subtree_has_boundary(node: &RouteNode) -> bool {
    node.middleware.is_some()
        || node.fallback.is_some()
        || node.intercept.is_some()
        || node.children.values().any(subtree_has_boundary)
}

// Whether this node or any descendant defines a `fallback.rs`.
fn subtree_has_fallback(node: &RouteNode) -> bool {
    node.fallback.is_some() || node.children.values().any(subtree_has_fallback)
}

// Report (once) any boundary subtree whose nest prefix contains a catch-all
// segment — axum forbids wildcards in a `nest` prefix, so such a directory
// can't own a `middleware.rs`/`fallback.rs`. `base` is the prefix accumulated
// since the enclosing boundary.
fn check_catchall(node: &RouteNode, base: &[String], errors: &mut TokenStream) {
    for (raw_name, child) in &node.children {
        let mut child_base = base.to_vec();
        child_base.push(dir_url_segment(raw_name));

        if subtree_has_boundary(child) {
            if has_catchall(&child_base) {
                let prefix = url_from(&child_base);
                errors.extend(quote! {
                    compile_error!(concat!(
                        "a `middleware.rs`/`fallback.rs` subtree cannot be nested at a ",
                        "catch-all path ('", #prefix, "'): axum forbids wildcards in a `nest` prefix. ",
                        "Move the middleware/fallback out of the catch-all directory."
                    ));
                });
            }
            // Inside a boundary the prefix resets (it becomes a new mount point).
            check_catchall(child, &[], errors);
        } else {
            check_catchall(child, &child_base, errors);
        }
    }
}

// Whether this node or any descendant has a state-aware (`router, state`)
// `middleware.rs` or `fallback.rs`. When true the router can only be built via
// `into_router_with_state`, so `into_router()` is not generated.
fn tree_needs_state(node: &RouteNode) -> bool {
    node.middleware == Some(MiddlewareKind::Stateful)
        || node.fallback == Some(MiddlewareKind::Stateful)
        || node.intercept.as_ref().is_some_and(|s| s.stateful)
        || node.children.values().any(tree_needs_state)
}

// A resolved fallback to apply to a router: the module path segments of its
// `fallback` fn (ending in "fallback") plus how it consumes state.
type FallbackRef = (Vec<String>, MiddlewareKind);

// Emit `router = <ns>::<path>::fallback(router[, state.clone()]);`.
fn apply_fallback_tokens(mod_namespace: &syn::Path, fb: &FallbackRef) -> TokenStream {
    let (segments, kind) = fb;
    let path = generate_mod_path_tokens(segments);
    match kind {
        MiddlewareKind::Stateless => quote! {
            router = #mod_namespace::#path::fallback(router);
        },
        MiddlewareKind::Stateful => quote! {
            router = #mod_namespace::#path::fallback(router, state.clone());
        },
    }
}

// Emit route registrations for `node` onto an in-scope `router` binding, using
// URL paths relative to the enclosing nest (`base` = segments accumulated since
// the last boundary). This dir's own `route.rs` is registered first, then each
// child: a boundary child is `nest`ed as its own sub-router; a boundary-free
// child is inlined flat with its segment appended to `base`. `current_fb` is the
// fallback in scope for this router, threaded into nested boundary children.
fn emit_into(
    node: &RouteNode,
    base: &[String],
    current_fb: &Option<FallbackRef>,
    mod_namespace: &syn::Path,
    state_type: &syn::Type,
) -> TokenStream {
    let mut body = TokenStream::new();

    if let Some((mod_path, methods)) = &node.route {
        if !methods.is_empty() {
            let method_router = method_router(mod_namespace, mod_path, methods);
            let path = url_from(base);
            body.extend(quote! {
                router = router.route(#path, #method_router);
            });
        }
    }

    for (raw_name, child) in &node.children {
        let mut child_base = base.to_vec();
        child_base.push(dir_url_segment(raw_name));

        if subtree_has_boundary(child) {
            // A catch-all prefix can't be a `nest` point (axum panics); this is
            // reported once by `check_catchall` in `router_impl`. Skip emitting
            // the nest so we don't generate code that would panic at runtime.
            if has_catchall(&child_base) {
                continue;
            }
            let prefix = url_from(&child_base);
            let child_router = boundary_router(child, current_fb, mod_namespace, state_type);
            body.extend(quote! {
                router = router.nest(#prefix, #child_router);
            });
        } else {
            body.extend(emit_into(child, &child_base, current_fb, mod_namespace, state_type));
        }
    }

    body
}

// Extract `T` from a `State<T>` type (the first type generic argument), if any.
fn state_inner_type(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(tp) = ty else {
        return None;
    };
    let seg = tp.path.segments.last()?;
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    args.args.iter().find_map(|a| match a {
        syn::GenericArgument::Type(t) => Some(t),
        _ => None,
    })
}

// Reproduce an intercept parameter type for the generated wrapper, fully
// qualifying the two types the macro recognises so the wrapper compiles at the
// `#[folder_router]` invocation site even when the user only imported them inside
// `intercept.rs`: the forwarded `Request` becomes `axum::extract::Request`, and
// `State<T>` becomes `axum::extract::State<T>` (its inner `T` — typically the
// macro's state type — must be nameable at the site, which it is). Any other
// extractor is emitted verbatim, so its type must be nameable at the invocation
// site too: import it there or write it fully qualified in the signature.
fn requalify_intercept_type(ty: &syn::Type) -> TokenStream {
    if is_request_type(ty) {
        return quote! { axum::extract::Request };
    }
    if is_state_type(ty) {
        if let Some(inner) = state_inner_type(ty) {
            return quote! { axum::extract::State<#inner> };
        }
    }
    quote! { #ty }
}

// Build a self-contained `Router` for a boundary `node`, mounted (by the caller)
// at its prefix. Routes inside are relative (start at "/"). Applies this
// subtree's fallback, then its intercept, then its middleware. Used for the root
// and every nested boundary.
//
// Fallback resolution is explicit (not left to axum's nesting, which lets an
// ancestor fallback silently override a deeper one through a fallback-less
// layer): use this dir's own `fallback.rs`, else the nearest ancestor's
// (`inherited_fb`, re-applied), else — if a descendant defines one — a
// synthesized 404 so this router isn't transparent.
fn boundary_router(
    node: &RouteNode,
    inherited_fb: &Option<FallbackRef>,
    mod_namespace: &syn::Path,
    state_type: &syn::Type,
) -> TokenStream {
    let own_fb: Option<FallbackRef> = node.fallback.map(|kind| {
        let mut segments = node.mod_segments.clone();
        segments.push("fallback".to_string());
        (segments, kind)
    });
    let effective_fb = own_fb.or_else(|| inherited_fb.clone());

    // Children (and any nested boundaries within them) resolve to this router's
    // effective fallback unless they define their own.
    let mut body = emit_into(node, &[], &effective_fb, mod_namespace, state_type);

    if let Some(fb) = &effective_fb {
        body.extend(apply_fallback_tokens(mod_namespace, fb));
    } else if node.children.values().any(subtree_has_fallback) {
        // No fallback in scope, but a descendant defines one: make this router
        // non-transparent so the descendant's fallback isn't overridden.
        body.extend(quote! {
            router = router.fallback(|| async { axum::http::StatusCode::NOT_FOUND });
        });
    }

    // Per-request interception, applied below the fallback (so it gates the
    // fallback too) and above any same-folder `middleware` (which wraps it). The
    // user writes only the decision returning `ControlFlow<Response, Request>`;
    // the macro generates the layer. Always `.layer` (never `route_layer`), so a
    // guard can never silently skip an unmatched/fallback-served path.
    if let Some(sig) = &node.intercept {
        let mut segments = node.mod_segments.clone();
        segments.push("intercept".to_string());
        let intercept_path = generate_mod_path_tokens(&segments);

        // Reproduce the intercept's extractor parameters on the generated layer,
        // forwarding them positionally (the user's own param patterns destructure
        // them). The `Continue(req)` arm rebinds `req` from the `ControlFlow`
        // payload, so the macro never needs to know which input was the request.
        let arg_idents: Vec<_> = (0..sig.params.len())
            .map(|i| format_ident!("__arg{}", i))
            .collect();
        let arg_types: Vec<TokenStream> = sig.params.iter().map(requalify_intercept_type).collect();

        let layer = if sig.stateful {
            quote! { axum::middleware::from_fn_with_state(state.clone(), __folder_router_intercept) }
        } else {
            quote! { axum::middleware::from_fn(__folder_router_intercept) }
        };

        body.extend(quote! {
            async fn __folder_router_intercept(
                #( #arg_idents: #arg_types, )*
                next: axum::middleware::Next,
            ) -> axum::response::Response {
                match #mod_namespace::#intercept_path::intercept(#( #arg_idents ),*).await {
                    ::core::ops::ControlFlow::Continue(req) => next.run(req).await,
                    ::core::ops::ControlFlow::Break(resp) => resp,
                }
            }
            router = router.layer(#layer);
        });
    }

    if let Some(kind) = node.middleware {
        let mut segments = node.mod_segments.clone();
        segments.push("middleware".to_string());
        let middleware_path = generate_mod_path_tokens(&segments);
        body.extend(match kind {
            MiddlewareKind::Stateless => quote! {
                router = #mod_namespace::#middleware_path::middleware(router);
            },
            MiddlewareKind::Stateful => quote! {
                router = #mod_namespace::#middleware_path::middleware(router, state.clone());
            },
        });
    }

    quote! {
        {
            let mut router = axum::Router::<#state_type>::new();
            #body
            router
        }
    }
}

pub fn router_impl(
    errors: &mut TokenStream,
    args: &parse::FolderRouterArgs,
    item: &parse::FolderRouterItem,
    routes: &parse::FolderRouterRoutes,
) -> TokenStream {
    let struct_name = item.struct_name();
    let state_type = args.state_type.clone();
    let mod_namespace = item.module_namespace();

    let root = build_route_tree(routes);

    if !has_any_registration(&root) {
        errors.extend(quote! {
            compile_error!(concat!(
                "No routes defined in your route.rs's !\n",
                "Ensure that at least one `pub async fn` named after an HTTP verb is defined. (e.g. get, post, put, delete)"
            ));
        });
    }

    check_catchall(&root, &[], errors);

    let router_expr = boundary_router(&root, &None, &mod_namespace, &state_type);

    // `into_router()` (no state value) can't satisfy state-aware middleware, and
    // silently skipping it would be an auth-bypass footgun — so when any exists,
    // only `into_router_with_state` is generated.
    let into_router_method = if tree_needs_state(&root) {
        TokenStream::new()
    } else {
        quote! {
            pub fn into_router() -> axum::Router<#state_type> {
                #router_expr
            }
        }
    };

    quote! {
        impl #struct_name {
            pub fn into_router_with_state(state: #state_type) -> axum::Router {
                let router = #router_expr;
                router.with_state(state)
            }

            #into_router_method
        }
    }
}

pub fn module_tree(
    args: &parse::FolderRouterArgs,
    item: &parse::FolderRouterItem,
    routes: &parse::FolderRouterRoutes,
) -> TokenStream {
    let base_path_lit = LitStr::new(
        &args.abs_path().to_string_lossy(),
        proc_macro2::Span::call_site(),
    );

    let mod_namespace = item.module_namespace();

    let mod_str = mod_namespace.to_token_stream().to_string();
    let mut root = ModuleDir::new(&mod_str);
    for (_route_path, rel_path) in routes.routes() {
        root.add_to_module_tree(rel_path);
    }
    for (_mw_path, rel_path, _kind) in routes.middleware() {
        root.add_to_module_tree(rel_path);
    }
    for (_fb_path, rel_path, _kind) in routes.fallback() {
        root.add_to_module_tree(rel_path);
    }
    for (_ic_path, rel_path, _sig) in routes.intercept() {
        root.add_to_module_tree(rel_path);
    }

    let mod_hierarchy = generate_module_hierarchy(&root);
    quote! {
        #[path = #base_path_lit]
        mod #mod_namespace {
            #mod_hierarchy
        }
    }
}
