use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, FnArg, GenericArgument, Ident, ItemFn, Pat, PathArguments, ReturnType, Type,
    TypeParamBound,
};

/// Turns a function returning `impl Stream<Item = T>` into a Tauri command
/// that forwards the stream to the `onEvent` channel the frontend passes, and
/// returns the subscription's resource id. A function returning
/// `Result<impl Stream<Item = T>, E>` becomes a command that fails with `E`
/// when the stream can't be set up.
#[proc_macro_attribute]
pub fn subscription(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    expand(function)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(function: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let output = Output::parse(&function.sig.output)?;
    let arg_names = function
        .sig
        .inputs
        .iter()
        .map(arg_name)
        .collect::<syn::Result<Vec<_>>>()?;
    let inputs = function.sig.inputs.iter();
    let name = &function.sig.ident;
    let vis = &function.vis;
    let attrs = &function.attrs;
    let await_stream = function.sig.asyncness.map(|_| quote!(.await));

    let stream_fn_name = format_ident!("{name}_stream");
    let mut stream_fn = function.clone();
    stream_fn.sig.ident = stream_fn_name.clone();
    stream_fn.vis = syn::Visibility::Inherited;
    stream_fn.attrs.clear();

    let make_stream = quote!(#stream_fn_name(#(#arg_names),*)#await_stream);
    let (item, error, stream) = match output {
        Output::Stream { item } => (item, quote!(::std::string::String), make_stream),
        Output::Fallible { item, error } => (item, quote!(#error), quote!(#make_stream?)),
    };

    Ok(quote! {
        #stream_fn

        #(#attrs)*
        #[::tauri::command]
        #vis async fn #name(
            #(#inputs,)*
            on_event: ::tauri_plugin_subscriptions::Subscriber<#item, ::tauri::Wry>,
        ) -> ::std::result::Result<::tauri::ResourceId, #error> {
            ::std::result::Result::Ok(on_event.subscribe(#stream))
        }
    })
}

enum Output<'a> {
    Stream { item: &'a Type },
    Fallible { item: &'a Type, error: &'a Type },
}

impl<'a> Output<'a> {
    fn parse(output: &'a ReturnType) -> syn::Result<Self> {
        let error = || {
            syn::Error::new_spanned(
                output,
                "a #[subscription] must return `impl Stream<Item = T>` or `Result<impl Stream<Item = T>, E>`",
            )
        };
        let ReturnType::Type(_, ty) = output else {
            return Err(error());
        };
        if let Some(item) = stream_item(ty) {
            return Ok(Self::Stream { item });
        }
        let Some([ok, error_type]) = result_types(ty) else {
            return Err(error());
        };
        let item = stream_item(ok).ok_or_else(error)?;
        Ok(Self::Fallible {
            item,
            error: error_type,
        })
    }
}

/// `T` in `impl Stream<Item = T>`.
fn stream_item(ty: &Type) -> Option<&Type> {
    let Type::ImplTrait(impl_trait) = ty else {
        return None;
    };
    impl_trait
        .bounds
        .iter()
        .filter_map(|bound| match bound {
            TypeParamBound::Trait(bound) => bound.path.segments.last(),
            _ => None,
        })
        .filter(|segment| segment.ident == "Stream")
        .flat_map(|segment| generic_arguments(&segment.arguments))
        .find_map(|argument| match argument {
            GenericArgument::AssocType(assoc) if assoc.ident == "Item" => Some(&assoc.ty),
            _ => None,
        })
}

/// `[T, E]` in `Result<T, E>`.
fn result_types(ty: &Type) -> Option<[&Type; 2]> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "Result" {
        return None;
    }
    let mut types = generic_arguments(&segment.arguments).filter_map(|argument| match argument {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    Some([types.next()?, types.next()?])
}

fn generic_arguments(arguments: &PathArguments) -> impl Iterator<Item = &GenericArgument> {
    match arguments {
        PathArguments::AngleBracketed(arguments) => Some(arguments.args.iter()),
        _ => None,
    }
    .into_iter()
    .flatten()
}

fn arg_name(arg: &FnArg) -> syn::Result<&Ident> {
    match arg {
        FnArg::Typed(typed) => match typed.pat.as_ref() {
            Pat::Ident(pat) => Ok(&pat.ident),
            pat => Err(syn::Error::new_spanned(
                pat,
                "#[subscription] arguments must be plain names",
            )),
        },
        FnArg::Receiver(receiver) => Err(syn::Error::new_spanned(
            receiver,
            "#[subscription] can't take `self`",
        )),
    }
}
