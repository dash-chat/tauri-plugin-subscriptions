use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, FnArg, GenericArgument, Ident, ItemFn, Pat, PathArguments, ReturnType, Type,
    TypeParamBound,
};

/// Turns a function returning `impl Stream<Item = T>` into a Tauri command
/// that forwards the stream to the `onEvent` channel the frontend passes, and
/// returns the subscription's resource id.
#[proc_macro_attribute]
pub fn subscription(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    expand(function)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(function: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let item = stream_item(&function.sig.output)?;
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

    Ok(quote! {
        #stream_fn

        #(#attrs)*
        #[::tauri::command]
        #vis async fn #name(
            #(#inputs,)*
            on_event: ::tauri_plugin_subscriptions::Subscriber<#item, ::tauri::Wry>,
        ) -> ::std::result::Result<::tauri::ResourceId, ::std::string::String> {
            ::std::result::Result::Ok(on_event.subscribe(#stream_fn_name(#(#arg_names),*)#await_stream))
        }
    })
}

fn stream_item(output: &ReturnType) -> syn::Result<&Type> {
    let error = || {
        syn::Error::new_spanned(
            output,
            "a #[subscription] must return `impl Stream<Item = T>`",
        )
    };
    let ReturnType::Type(_, ty) = output else {
        return Err(error());
    };
    let Type::ImplTrait(impl_trait) = ty.as_ref() else {
        return Err(error());
    };
    impl_trait
        .bounds
        .iter()
        .filter_map(|bound| match bound {
            TypeParamBound::Trait(bound) => bound.path.segments.last(),
            _ => None,
        })
        .filter(|segment| segment.ident == "Stream")
        .filter_map(|segment| match &segment.arguments {
            PathArguments::AngleBracketed(arguments) => Some(arguments.args.iter()),
            _ => None,
        })
        .flatten()
        .find_map(|argument| match argument {
            GenericArgument::AssocType(assoc) if assoc.ident == "Item" => Some(&assoc.ty),
            _ => None,
        })
        .ok_or_else(error)
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
