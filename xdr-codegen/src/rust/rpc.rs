//! RPC programs: for each version, a service trait, a server-side
//! dispatcher and a client, for `thruput_rpc_api`.
//!
//! For `program P { version V { ... } = 1; } = 0x20000001;` the module
//! defines:
//!
//! - `trait P_V`, with an async method per procedure. Each handler replies
//!   with anything that encodes like the reply type.
//! - `P_V_Dispatcher<H>`, an `RpcHandler` that decodes a call's arguments,
//!   calls the handler `H: P_V` and encodes its reply.
//! - `P_V_Client`, with an async method per procedure that encodes the
//!   arguments, sends the call and decodes the reply. Scalar arguments go
//!   by value; others are borrowed as anything that encodes like them.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::Gen;
use super::comment::{self, Comment};
use crate::ast::*;

impl Gen<'_> {
    pub(super) fn program(&mut self, program: &Program, comment: &Comment) {
        let prog_value = self.value(&program.value);
        for version in &program.versions {
            let base = format!("{}_{}", program.name.name, version.name.name);
            self.version(&base, &prog_value, version, comment);
        }
    }

    fn version(
        &mut self,
        base: &str,
        prog_value: &TokenStream,
        version: &Version,
        comment: &Comment,
    ) {
        let trait_name = format_ident!("{base}");
        let dispatcher = format_ident!("{base}_Dispatcher");
        let client = format_ident!("{base}_Client");
        let vers_value = self.value(&version.value);
        let api = quote! { ::thruput_rpc_api };

        let mut trait_fns = Vec::new();
        let mut dispatch_arms = Vec::new();
        let mut client_fns = Vec::new();
        for proc in &version.procs {
            let name = self.name(&proc.name);
            let number = self.value(&proc.value);
            let result = match &proc.result {
                Some(t) => self.type_spec(t, "", proc.name.span, Default::default()),
                None => quote! { () },
            };
            let args: Vec<TokenStream> = proc
                .args
                .iter()
                .map(|t| self.type_spec(t, "", proc.name.span, Default::default()))
                .collect();
            let arg_names: Vec<_> = (0..args.len()).map(|i| format_ident!("arg{i}")).collect();
            let indices = (0..args.len()).map(syn::Index::from);
            let doc = Comment::parse(&proc.comment).attrs();

            trait_fns.push(quote! {
                #doc
                fn #name(&self, #(#arg_names: #args),*)
                    -> impl ::core::future::Future<
                        Output = Result<impl xdrs::EncodesAs<#result>, #api::ProgramError>,
                    > + Send;
            });

            dispatch_arms.push(quote! {
                #number => {
                    let args: (#(#args,)*) = xdrs::txs::decode(data)
                        .map_err(|_| #api::RpcError::GarbageArguments)?;
                    let res = self.inner.#name(#(args.#indices),*).await
                        .map_err(|_| #api::RpcError::SystemError)?;
                    xdrs::txs::encode(&res).map_err(|_| #api::RpcError::SystemError)
                }
            });

            let client_args = proc.args.iter().zip(&args).map(|(t, rust)| {
                if is_scalar(t) {
                    rust.clone()
                } else {
                    quote! { &(impl xdrs::EncodesAs<#rust> + ?Sized) }
                }
            });
            let safety = comment::plain(&[
                "SAFETY: the call future is awaited within this method, so it lives in",
                "this method's future along with `out` and `decode`. Once that is",
                "polled it is pinned, so neither can be freed without the call future",
                "being dropped first. Decoding never blocks.",
            ]);
            client_fns.push(quote! {
                #safety
                pub async fn #name(&self, #(#arg_names: #client_args),*) -> Result<#result, #api::RpcError> {
                    // Encode the arguments straight into the call buffer, so
                    // the driver frames the call in place.
                    let mut call = self.handle.call_buf();
                    call.append_with(|buf| {
                        let mut w = xdrs::txs::buf_writer::BufWriter::from(::core::mem::take(buf));
                        let r = xdrs::write_xdr(&(#(#arg_names),*), &mut w);
                        *buf = w.into();
                        r
                    })
                    .map_err(|_| #api::RpcError::GarbageArguments)?;
                    // Decode the reply wherever the sender has it, without
                    // first copying it into an owned response.
                    let mut out = ::core::option::Option::None;
                    let mut decode = |reply: &[u8]| -> ::core::result::Result<(), #api::RpcError> {
                        out = ::core::option::Option::Some(
                            xdrs::txs::decode(reply).map_err(|_| #api::RpcError::SystemError)?,
                        );
                        Ok(())
                    };
                    unsafe { self.handle.send_call_decoded(Self::prog_vers(), #number, call, &mut decode) }.await?;
                    out.ok_or(#api::RpcError::SystemError)
                }
            });
        }

        let doc = comment.attrs();
        self.items.push(quote! {
            #doc
            /// Each handler replies with anything that encodes like the reply type.
            /// Implementations can be plain `async fn`s returning a concrete reply type
            /// under `#[allow(refining_impl_trait)]`.
            #[allow(non_camel_case_types)]
            pub trait #trait_name: Send + Sync {
                #(#trait_fns)*
            }
        });
        // The handler's type parameter shares a scope with the schema's type
        // names; names starting with `__` are reserved in the C++ x23c
        // generates, so no schema uses this one.
        self.items.push(quote! {
            #[allow(non_camel_case_types)]
            pub struct #dispatcher<__Handler> {
                inner: Box<__Handler>,
            }
        });
        self.items.push(quote! {
            impl<__Handler: #trait_name> #dispatcher<__Handler> {
                pub fn new(inner: Box<__Handler>) -> Self {
                    Self { inner }
                }
            }
        });
        self.items.push(quote! {
            #[async_trait::async_trait]
            impl<__Handler: #trait_name> #api::RpcHandler for #dispatcher<__Handler> {
                fn prog_vers(&self) -> #api::ProgVers {
                    #api::ProgVers::new(#prog_value, #vers_value)
                }
                async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, #api::RpcError> {
                    match proc {
                        #(#dispatch_arms)*
                        _ => Err(#api::RpcError::InvalidProcedure),
                    }
                }
            }
        });
        self.items.push(quote! {
            #[allow(non_camel_case_types)]
            #[derive(Clone)]
            pub struct #client {
                handle: ::std::sync::Arc<dyn #api::RpcSender>,
            }
        });
        self.items.push(quote! {
            impl #client {
                pub fn new(handle: ::std::sync::Arc<dyn #api::RpcSender>) -> Self {
                    Self { handle }
                }
                fn prog_vers() -> #api::ProgVers {
                    #api::ProgVers::new(#prog_value, #vers_value)
                }
                #(#client_fns)*
            }
        });
        self.items.push(quote! {
            impl From<::std::sync::Arc<dyn #api::RpcSender>> for #client {
                fn from(handle: ::std::sync::Arc<dyn #api::RpcSender>) -> Self {
                    Self::new(handle)
                }
            }
        });
    }
}

/// Whether a client passes an argument of type `t` by value.
fn is_scalar(t: &TypeSpec) -> bool {
    matches!(
        t,
        TypeSpec::Int
            | TypeSpec::UnsignedInt
            | TypeSpec::Hyper
            | TypeSpec::UnsignedHyper
            | TypeSpec::Bool
            | TypeSpec::Float
            | TypeSpec::Double
    )
}
