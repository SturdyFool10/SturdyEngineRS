use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

/// Derive macro that generates boilerplate for push constant structs.
///
/// This macro expands to:
/// - `#[repr(C)]` attribute
/// - `Copy` and `Clone` trait derives
/// - `bytemuck::Pod` and `bytemuck::Zeroable` trait implementations
///
/// # Example
///
/// ```ignore
/// #[derive(PushConstants)]
/// struct MyConstants {
///     time: f32,
///     resolution: [f32; 2],
/// }
/// ```
///
/// Which expands to roughly:
///
/// ```ignore
/// #[repr(C)]
/// #[derive(Copy, Clone)]
/// struct MyConstants {
///     time: f32,
///     resolution: [f32; 2],
/// }
///
/// unsafe impl bytemuck::Pod for MyConstants {}
/// unsafe impl bytemuck::Zeroable for MyConstants {}
/// ```
#[proc_macro_derive(PushConstants)]
pub fn push_constants_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let expanded = quote! {
        #[repr(C)]
        #[derive(Copy, Clone)]
        #input

        unsafe impl bytemuck::Pod for #name {}
        unsafe impl bytemuck::Zeroable for #name {}
    };

    TokenStream::from(expanded)
}
