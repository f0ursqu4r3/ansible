use std::collections::HashMap;
use std::path::Path;

use proc_macro2::Span;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Comma;
use syn::visit::Visit;
use syn::{
    ExprField, ExprPath, File, FnArg, ImplItem, ImplItemFn, Item, ItemFn, ItemImpl, ItemUse, Local,
    Member, Pat, PatIdent, PatType, Receiver, TypePath, UseTree,
};

use crate::model::types::{DefinitionLocation, NameReference};

fn module_hint(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string()
}

fn span_position(span: Span) -> Option<(usize, usize)> {
    let start = span.start();
    Some((start.line.saturating_sub(1), start.column))
}

#[derive(Default)]
struct Scope {
    defs: HashMap<String, DefinitionLocation>,
}

struct SymbolVisitor<'a> {
    path: &'a Path,
    module: String,
    scopes: Vec<Scope>,
    glob_imports: Vec<DefinitionLocation>,
    refs: Vec<NameReference>,
}

impl<'a> SymbolVisitor<'a> {
    fn new(path: &'a Path) -> Self {
        Self {
            path,
            module: module_hint(path),
            scopes: vec![Scope::default()],
            glob_imports: Vec::new(),
            refs: Vec::new(),
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        if self.scopes.is_empty() {
            self.scopes.push(Scope::default());
        }
    }

    fn add_local_def(
        &mut self,
        name: &str,
        span: Span,
        module_path: String,
    ) -> Option<DefinitionLocation> {
        let (line, col) = span_position(span)?;
        let loc = DefinitionLocation {
            file: self.path.to_path_buf(),
            module_path,
            line,
            col,
        };
        if let Some(scope) = self.scopes.last_mut() {
            scope.defs.insert(name.to_string(), loc.clone());
        }
        Some(loc)
    }

    fn add_import_def(
        &mut self,
        name: &str,
        span: Span,
        module_path: String,
        is_glob: bool,
    ) -> Option<DefinitionLocation> {
        let loc = self.add_local_def(name, span, module_path)?;
        if is_glob {
            self.glob_imports.push(loc.clone());
        } else {
            self.glob_imports
                .retain(|g| g.module_path != loc.module_path);
        }
        Some(loc)
    }

    fn resolve(&self, name: &str) -> Option<DefinitionLocation> {
        for scope in self.scopes.iter().rev() {
            if let Some(def) = scope.defs.get(name) {
                return Some(def.clone());
            }
        }
        None
    }

    fn record_ref(&mut self, name: &str, span: Span) {
        let (line, col) = match span_position(span) {
            Some(pos) => pos,
            None => return,
        };
        let mut target = self.resolve(name);
        if target.is_none() {
            if let Some(glob) = self.glob_imports.last() {
                target = Some(glob.clone());
            }
        }
        self.refs.push(NameReference {
            name: name.to_string(),
            line,
            col,
            len: name.len(),
            target,
        });
    }

    fn add_receiver(&mut self, recv: &Receiver) {
        let span = recv.span();
        let _ = self.add_local_def("self", span, format!("{}::self", self.module));
    }

    fn add_pat_idents(&mut self, pat: &Pat) {
        match pat {
            Pat::Ident(PatIdent { ident, .. }) => {
                let name = ident.to_string();
                let _ = self.add_local_def(&name, ident.span(), format!("{}::{name}", self.module));
            }
            Pat::Type(PatType { pat, .. }) => self.add_pat_idents(pat),
            Pat::Tuple(tuple) => {
                for p in &tuple.elems {
                    self.add_pat_idents(p);
                }
            }
            Pat::Struct(st) => {
                for field in &st.fields {
                    self.add_pat_idents(&field.pat);
                }
            }
            Pat::TupleStruct(ts) => {
                for p in &ts.elems {
                    self.add_pat_idents(p);
                }
            }
            Pat::Slice(slice) => {
                for p in &slice.elems {
                    self.add_pat_idents(p);
                }
            }
            Pat::Reference(r) => self.add_pat_idents(&r.pat),
            Pat::Or(or) => {
                for p in &or.cases {
                    self.add_pat_idents(p);
                }
            }
            _ => {}
        }
    }

    fn add_fn_inputs(&mut self, inputs: &Punctuated<FnArg, Comma>) {
        for arg in inputs {
            match arg {
                FnArg::Receiver(recv) => self.add_receiver(recv),
                FnArg::Typed(pt) => self.add_pat_idents(&pt.pat),
            }
        }
    }

    fn collect_use_tree(&mut self, prefix: &str, tree: &UseTree) {
        match tree {
            UseTree::Path(p) => {
                let mut next = prefix.to_string();
                next.push_str(&p.ident.to_string());
                next.push_str("::");
                self.collect_use_tree(&next, &p.tree);
            }
            UseTree::Name(n) => {
                let path = format!("{}{}", prefix, n.ident);
                let _ = self.add_import_def(
                    &n.ident.to_string(),
                    n.ident.span(),
                    format!("use {}", path),
                    false,
                );
            }
            UseTree::Rename(r) => {
                let path = format!("{}{}", prefix, r.ident);
                let _ = self.add_import_def(
                    &r.rename.to_string(),
                    r.rename.span(),
                    format!("use {}", path),
                    false,
                );
            }
            UseTree::Glob(g) => {
                let name = format!("{}*", prefix.trim_end_matches("::"));
                let _ = self.add_import_def(
                    &name,
                    g.star_token.span(),
                    format!("use {}*", prefix.trim_end_matches("::")),
                    true,
                );
            }
            UseTree::Group(g) => {
                for entry in &g.items {
                    self.collect_use_tree(prefix, entry);
                }
            }
        }
    }

    fn with_scope<F: FnOnce(&mut Self)>(&mut self, f: F) {
        self.push_scope();
        f(self);
        self.pop_scope();
    }
}

impl<'ast> Visit<'ast> for SymbolVisitor<'_> {
    fn visit_file(&mut self, node: &'ast File) {
        for item in &node.items {
            match item {
                Item::Fn(func) => self.visit_item_fn(func),
                Item::Impl(item_impl) => self.visit_item_impl(item_impl),
                Item::Use(item_use) => self.visit_item_use(item_use),
                _ => syn::visit::visit_item(self, item),
            }
        }
    }

    fn visit_item_use(&mut self, node: &'ast ItemUse) {
        self.collect_use_tree("", &node.tree);
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        self.with_scope(|v| {
            v.add_fn_inputs(&node.sig.inputs);
            syn::visit::visit_block(v, &node.block);
        });
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        for item in &node.items {
            match item {
                ImplItem::Fn(ImplItemFn { sig, block, .. }) => {
                    self.with_scope(|v| {
                        v.add_fn_inputs(&sig.inputs);
                        syn::visit::visit_block(v, block);
                    });
                }
                _ => syn::visit::visit_impl_item(self, item),
            }
        }
    }

    fn visit_local(&mut self, node: &'ast Local) {
        self.add_pat_idents(&node.pat);
        syn::visit::visit_local(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast ExprPath) {
        if let Some(seg) = node.path.segments.last() {
            self.record_ref(&seg.ident.to_string(), seg.ident.span());
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_expr_field(&mut self, node: &'ast ExprField) {
        if let Member::Named(ident) = &node.member {
            self.record_ref(&ident.to_string(), ident.span());
        }
        syn::visit::visit_expr_field(self, node);
    }

    fn visit_type_path(&mut self, node: &'ast TypePath) {
        if let Some(seg) = node.path.segments.last() {
            self.record_ref(&seg.ident.to_string(), seg.ident.span());
        }
        syn::visit::visit_type_path(self, node);
    }
}

pub fn analyze_rust_symbols(path: &Path, content: &str) -> Vec<NameReference> {
    let Ok(file) = syn::parse_file(content) else {
        return Vec::new();
    };
    let mut visitor = SymbolVisitor::new(path);
    visitor.visit_file(&file);
    visitor.refs
}
