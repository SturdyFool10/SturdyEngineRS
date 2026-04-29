use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

/// Attribute macro that generates boilerplate for push constant structs.
///
/// Adds `#[repr(C)]`, `#[derive(Copy, Clone)]`, and `bytemuck::Pod` /
/// `bytemuck::Zeroable` impls to the annotated struct.
///
/// # Example
///
/// ```ignore
/// use sturdy_engine::push_constants;
///
/// #[push_constants]
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
#[proc_macro_attribute]
pub fn push_constants(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let name = &input.ident;

    // The attribute macro replaces the item, so we re-emit the struct with
    // the added attributes prepended.  `#input` here does NOT include the
    // `#[push_constants]` attribute itself — it was consumed by the macro.
    let expanded = quote! {
        #[repr(C)]
        #[derive(Copy, Clone)]
        #input

        unsafe impl bytemuck::Pod for #name {}
        unsafe impl bytemuck::Zeroable for #name {}
    };

    TokenStream::from(expanded)
}

/// Deprecated no-op retained for compatibility with local experiments.
///
/// `tools/panic-audit.py` no longer recognizes this attribute. Use a
/// `//panic allowed, reason = "<reason>"` comment immediately before the
/// reviewed statement instead.
///
/// # Usage
///
/// Apply it to the `let` statement or expression statement that contains the
/// panic site.  An optional string literal gives the reason.
///
/// ```ignore
/// //panic allowed, reason = "poisoned mutex is unrecoverable"
/// let guard = self.inner.lock().expect("device mutex poisoned");
///
/// //panic allowed, reason = "poisoned mutex is unrecoverable"
/// let guard = self.inner
///     .lock()
///     .expect("device mutex poisoned");
/// ```
#[proc_macro_attribute]
pub fn allow_panic(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // Intentional no-op: return the annotated item completely unchanged.
    item
}
