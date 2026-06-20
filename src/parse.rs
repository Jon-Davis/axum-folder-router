use std::{
    fs,
    path::{Path, PathBuf},
};

use quote::ToTokens;
use syn::{
    parse::{Parse, ParseStream},
    parse_file,
    Ident,
    Item,
    LitStr,
    Result,
    Token,
    Visibility,
};

#[derive(Debug)]
pub struct FolderRouterArgs {
    pub path: String,
    pub state_type: Ident,
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
        let state_type = input.parse::<Ident>()?;

        Ok(FolderRouterArgs {
            path: path_lit.value(),
            state_type,
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

// Collect `route.rs`, `middleware.rs` and `fallback.rs` files recursively
pub fn collect_files(
    base_dir: &Path,
    dir: &Path,
    routes: &mut Vec<(PathBuf, PathBuf)>,
    middleware: &mut Vec<(PathBuf, PathBuf)>,
    fallback: &mut Vec<(PathBuf, PathBuf)>,
) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(std::result::Result::ok) {
            let path = entry.path();

            if path.is_dir() {
                collect_files(base_dir, &path, routes, middleware, fallback);
            } else if let Ok(rel_dir) = path.strip_prefix(base_dir) {
                match path.file_name().and_then(|n| n.to_str()) {
                    Some("route.rs") => routes.push((path.clone(), rel_dir.to_path_buf())),
                    Some("middleware.rs") => middleware.push((path.clone(), rel_dir.to_path_buf())),
                    Some("fallback.rs") => fallback.push((path.clone(), rel_dir.to_path_buf())),
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
}

impl FolderRouterRoutes {
    pub fn parse_from_path(errors: &mut proc_macro2::TokenStream, path: &Path) -> Self {
        let mut routes = Vec::new();
        let mut raw_middleware = Vec::new();
        let mut raw_fallback = Vec::new();
        collect_files(path, path, &mut routes, &mut raw_middleware, &mut raw_fallback);
        routes.sort();
        raw_middleware.sort();
        raw_fallback.sort();

        let path_cow = path.to_string_lossy();
        let path_str = path_cow.as_ref();

        if routes.is_empty() {
            errors.extend(quote::quote! {
                compile_error!(concat!("No route.rs files found in the specified directory: '",
                    #path_str,
                    "'. Make sure the path is correct and contains route.rs files."
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

        Self {
            routes,
            middleware,
            fallback,
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
}
