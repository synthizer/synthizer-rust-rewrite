use proc_macro::TokenStream;
use proc_macro_error::{abort, ResultExt};
use quote::quote;
use syn::parse_quote;
use syn::{
    parse::{Parse, ParseStream},
    Token,
};

#[derive(Clone)]
struct BindingArgs {
    binding_kind: LetOrConst,
    identifier: syn::Ident,
    branch: syn::Expr,
    slow: syn::Block,
    slow_ty: syn::Type,
    fast: syn::Block,
    fast_ty: syn::Type,

    /// Type of the expression. Differs from fast_ty for the if case.
    fast_expr_ty: syn::Type,

    /// Like fast_expr_ty but for slow.
    slow_expr_ty: syn::Type,
}

#[derive(Clone, derive_more::IsVariant, derive_more::Unwrap)]
enum LetOrConst {
    Let(syn::token::Let),
    Const(syn::token::Const),
}

#[derive(Clone)]
enum CondPattern {
    /// Equivalent to `let identifier = identifier`.
    SimpleIdentifier(syn::Ident),

    /// This pattern will be bound to a name, based on the expression type on the right.
    Binding(BindingArgs),
}

struct CondTree {
    patterns: Vec<CondPattern>,
    block: syn::ExprBlock,
}

impl Parse for LetOrConst {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(Token![let]) {
            Ok(LetOrConst::Let(input.parse()?))
        } else if input.peek(Token![const]) {
            Ok(LetOrConst::Const(input.parse()?))
        } else {
            abort!(input.span(), "Expected let or const");
        }
    }
}

impl Parse for BindingArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut branch: syn::Expr;
        let slow: syn::Block;
        let fast: syn::Block;

        let binding_kind: LetOrConst = input.parse()?;
        let identifier: syn::Ident = input.parse()?;

        let mut slow_ty = syn::parse_quote_spanned!(identifier.span() => _);
        let mut fast_ty = syn::parse_quote_spanned!(identifier.span() => _);
        let fast_expr_ty;
        let slow_expr_ty;

        if input.peek(Token![:]) {
            input.parse::<Token![:]>()?;
            let inner;
            syn::parenthesized!(inner in input);
            let mut tys = inner
                .parse_terminated::<syn::Type, Token![,]>(syn::Type::parse)?
                .into_pairs()
                .map(|x| x.into_value())
                .collect::<Vec<_>>();
            if tys.len() != 2 {
                abort!(
                    identifier,
                    "If specifying types, both sides must be specified: (ty1, ty2)"
                );
            }

            slow_ty = tys.pop().unwrap();
            fast_ty = tys.pop().unwrap();
        }

        if binding_kind.is_const()
            && (matches!(slow_ty, syn::Type::Infer(..)) || matches!(fast_ty, syn::Type::Infer(..)))
        {
            abort!(
                binding_kind.unwrap_const(),
                "const bindings must specify the types of both variants: const ident: (fast, slow)"
            );
        }

        input.parse::<Token![=]>()?;

        // Two cases, determined by if we start with `if`: if { expr} else { expr }, or treating the whole thing as
        // expr.  Let Syn parse them both, then match the enum.
        let intermediate_expr: syn::Expr = input.parse()?;

        // We need this for abort!, since we partially move out.
        let cloned_intermediate_expr = intermediate_expr.clone();

        if let syn::Expr::If(expr) = intermediate_expr {
            branch = *expr.cond;
            branch = syn::parse_quote!((#branch).evaluate_divergence().to_unit());

            fast_expr_ty = fast_ty;
            slow_expr_ty = slow_ty;
            fast_ty = parse_quote!(());
            slow_ty = parse_quote!(());

            fast = expr.then_branch;
            slow = match expr.else_branch {
                Some((_, child)) => match *child {
                    syn::Expr::Block(x) => x.block,
                    _ => {
                        abort!(child, "This must be a block expression");
                    }
                },
                None => abort!(
                    cloned_intermediate_expr,
                    "an else branch is always required"
                ),
            };
        } else {
            branch = parse_quote!((#intermediate_expr).evaluate_divergence());
            // these match up to the macro rendering, which uses __m_res as an intermediate identifier that is
            // immediately moved from.
            fast = syn::parse_quote!({ __m_res });
            slow = syn::parse_quote!({ __m_res });
            fast_expr_ty = fast_ty.clone();
            slow_expr_ty = slow_ty.clone();
        }

        Ok(BindingArgs {
            binding_kind,
            identifier,
            branch,
            slow,
            slow_ty,
            fast,
            fast_ty,
            fast_expr_ty,
            slow_expr_ty,
        })
    }
}

impl Parse for CondPattern {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // If the next token is an identifier, that's simple enough: we get SimpleIdentifier and are done.
        if input.peek(syn::Ident) {
            Ok(CondPattern::SimpleIdentifier(input.parse()?))
        } else if input.peek(Token![let]) || input.peek(Token![const]) {
            Ok(Self::Binding(input.parse()?))
        } else {
            abort!(
                input.span(),
                "patterns must be an identifier, or start with let or const"
            );
        }
    }
}

impl Parse for CondTree {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let patterns: Vec<CondPattern> = {
            let pat_list;
            syn::parenthesized!(pat_list in input);
            pat_list
                .parse_terminated::<_, Token![,]>(CondPattern::parse)?
                .into_pairs()
                .map(|x| x.into_value())
                .collect()
        };

        input.parse::<Token![=>]>()?;

        let block = input.parse()?;

        Ok(CondTree { patterns, block })
    }
}

fn render_one(pat: &CondPattern, child: &syn::Expr) -> syn::Expr {
    match pat {
        CondPattern::SimpleIdentifier(ident) => {
            parse_quote!(
                match {
                    use cond_tree::Divergence as _;
                    #ident.evaluate_divergence()
                } {
                    cond_tree::Cond::Fast(#ident) => #child,
                    cond_tree::Cond::Slow(#ident) => #child,
                }
            )
        }
        CondPattern::Binding(BindingArgs {
            binding_kind,
            identifier,
            branch,
            slow,
            slow_ty,
            fast,
            fast_ty,
            slow_expr_ty,
            fast_expr_ty,
        }) => {
            let binding_kw = match binding_kind {
                LetOrConst::Let(tok) => quote!(#tok),
                LetOrConst::Const(tok) => quote::quote!(#tok),
            };

            parse_quote!({
                    let __m_divergence: Cond<#slow_ty, #fast_ty> = #branch;
                    match __m_divergence {
                        Cond::Fast(__m_res) => {
                            #[allow(unused_braces)]
                            #binding_kw #identifier: #fast_expr_ty = #fast;
                            #child
                        },
                        Cond::Slow(__m_res) => {
                            #[allow(unused_braces)]
                            #binding_kw #identifier: #slow_expr_ty = #slow;
                            #child
                        },
                    }
            })
        }
    }
}

fn render_all(tree: CondTree) -> syn::Expr {
    let block = tree.block;
    let mut child: syn::Expr = parse_quote!(#block);

    // The way this works is that we proceed from right to left, rendering each pattern with the child block.
    //
    // each step doubles the number of match arms, because render_one renders the child twice.
    for p in tree.patterns.into_iter().rev() {
        child = render_one(&p, &child);
    }

    child
}

fn parse_header(header: syn::parse::ParseStream) -> syn::Result<Vec<CondPattern>> {
    Ok(header
        .parse_terminated::<_, Token![,]>(CondPattern::parse)?
        .into_pairs()
        .map(|x| x.into_value())
        .collect())
}

fn cond_tree_from_header_and_body(header: syn::Attribute, block: syn::ExprBlock) -> CondTree {
    let patterns = header.parse_args_with(parse_header).unwrap_or_abort();

    let ctree = CondTree { patterns, block };

    if ctree.patterns.is_empty() {
        let span = proc_macro::Span::call_site();
        abort!(span, "At least one pattern must be specified");
    }

    ctree
}

/// Mark a function as able to contain divergent expressions.
///
/// Once so marked it becomes possible to use [diverge] in this function.  Failure to marka function will end up with
/// compile-time errors about unrecognized attributes, or errors about experimental attributes on expressions.
///
/// See the module-level documentation for more.
#[proc_macro_attribute]
#[proc_macro_error::proc_macro_error]
pub fn diverge_fn(_attribute: TokenStream, input: TokenStream) -> TokenStream {
    use syn::fold::Fold;

    // the input attribute isn't anything we need to concern ourselves with: it is simply a marker.

    // This visitor knows how to do two things:
    //
    // 1. For any `ExprBlock` with the `#[diverge]` attribute, expand this to a divergent expression.  This happens
    //    before 2, because we handle the recursive visitation ourselves, and simply don't visit the attribute branch.
    // 2. For anything with attributes which aren't ours, error if it has a `#[diverge]` on it: this means that we hit a
    //    non-expression block.
    struct DivergeVisitor;

    const DIVERGE: &str = "diverge";

    impl Fold for DivergeVisitor {
        fn fold_attribute(&mut self, attrib: syn::Attribute) -> syn::Attribute {
            if let Some(ident) = attrib.path.get_ident() {
                if *ident == DIVERGE {
                    proc_macro_error::emit_error!(
                        ident,
                        "The diverge attribute may only be used on block expressions"
                    );
                }
            }

            attrib
        }

        fn fold_expr_block(&mut self, expr: syn::ExprBlock) -> syn::ExprBlock {
            let mut attrs = vec![];
            let mut diverge_attr = None;
            for a in expr.attrs {
                if let Some(ident) = a.path.get_ident() {
                    if *ident == DIVERGE {
                        if diverge_attr.is_some() {
                            proc_macro_error::emit_error!(a, "#[diverge] must only appear once");
                        }
                        diverge_attr = Some(a);
                        continue;
                    }
                }

                attrs.push(a);
            }

            let Some(diverge_attr) = diverge_attr else {
                return syn::ExprBlock { attrs, ..expr };
            };

            let block = syn::fold::fold_block(self, expr.block);
            let ctree = cond_tree_from_header_and_body(
                diverge_attr,
                syn::ExprBlock {
                    attrs,
                    block,
                    ..expr
                },
            );
            let ei = render_all(ctree);
            syn::parse_quote!({ #ei })
        }
    }

    let input: syn::ItemFn = syn::parse_macro_input!(input);
    let output = syn::fold::fold_item_fn(&mut DivergeVisitor, input);
    quote::quote!(#output).into()
}

/// An internal implementation detail. Punches out the traits for tuples.
///
/// impl_trait_for_tuples turns out not to be quite flexible enough to let us build an intermediate tuple, then unfold
/// over that tuple.
#[doc(hidden)]
#[proc_macro]
pub fn cond_tree_macro_tuples(input: TokenStream) -> TokenStream {
    let input: syn::LitInt = syn::parse_macro_input!(input);
    let num_elements: u64 = match input.base10_parse() {
        Ok(x) => x,
        Err(e) => return e.to_compile_error().into(),
    };

    let mut out: Vec<syn::ItemImpl> = vec![];

    for i in 1..=num_elements {
        let tparams: Vec<syn::Ident> = (1..=i)
            .map(|i| {
                let s = quote::format_ident!("T{}", i);
                parse_quote!(#s)
            })
            .collect();

        // Converts our tuple from a tuple of divergences to a tuple of conds.
        let divergence_to_conds: Vec<syn::Expr> = (0..i)
            .into_iter()
            .map(|i| {
                let i = proc_macro2::Literal::u64_unsuffixed(i);
                parse_quote!(self.#i.evaluate_divergence())
            })
            .collect();
        let divergence_to_conds: syn::Expr = parse_quote!((#(#divergence_to_conds),*,));

        let is_fast: Vec<syn::Expr> = (0..i)
            .into_iter()
            .map(|i| {
                let i = proc_macro2::Literal::u64_unsuffixed(i);
                parse_quote!(cond_tuple.#i.is_fast())
            })
            .collect();
        let is_fast: syn::Expr = parse_quote!(#(#is_fast)&&*);

        let unwrap_fast: Vec<syn::Expr> = (0..i)
            .into_iter()
            .map(|i| {
                let i = proc_macro2::Literal::u64_unsuffixed(i);
                parse_quote!(cond_tuple.#i.unwrap_fast())
            })
            .collect();
        let unwrap_fast: syn::Expr = parse_quote!((#(#unwrap_fast),*,));

        let become_slow: Vec<syn::Expr> = (0..i)
            .into_iter()
            .map(|i| {
                let i = proc_macro2::Literal::u64_unsuffixed(i);
                parse_quote!(cond_tuple.#i.become_slow().unwrap_slow())
            })
            .collect();
        let become_slow: syn::Expr = parse_quote!((#(#become_slow),*,));

        out.push(parse_quote!(
            impl<#(#tparams),*> Divergence for (#(#tparams),*,) where
                #(#tparams : Divergence),*,
                #(<#tparams as Divergence>::Slow: From<<#tparams as Divergence>::Fast>),* {
                type Fast = (#(#tparams::Fast),*,);
                type Slow = (#(#tparams::Slow),*,);

                fn evaluate_divergence(self) -> Cond<Self::Fast, Self::Slow> {
                    let cond_tuple = #divergence_to_conds;
                    let is_fast = #is_fast;

                    if is_fast {
                        Cond::Fast(#unwrap_fast)
                    } else {
                        Cond::Slow(#become_slow)
                    }
                }
            }
        ));
    }

    quote!(#(#out)*).into()
}
