use darling::*;
use syn::Result;
use crate::util::*;

#[derive(Clone, Debug, Default, FromDeriveInput)]
#[darling(attributes(xdr, xdrs), default)]
pub struct InputAttrs {
    /// Override the path to the `xdrs` crate, e.g. `#[xdr(crate = "my_sdk::xdrs")]`.
    #[darling(rename = "crate")]
    pub krate: Option<syn::Path>,

    /// The name for the XDR item, if different from the rust name
    pub name: Option<String>,

    /// The name of an existing XDR type to use as the discriminant, instead
    /// of generating one
    pub discrim_type: Option<String>,

    /// The name of the discriminant variable, if not "which"
    pub discrim_var: Option<String>,

    pub into_type: Option<syn::Type>,
    pub from_type: Option<syn::Type>,
    pub via: Option<syn::Type>,
}

#[derive(Clone, Debug, Default, FromField)]
#[darling(attributes(xdr, xdrs), default)]
pub struct FieldAttrs {
    // The name for the XDR item, if different from the rust name
    name: Option<String>,

    pub into_type: Option<syn::Type>,
    pub from_type: Option<syn::Type>,
    pub via: Option<syn::Type>,
}

#[derive(Clone, Debug, Default, FromVariant)]
#[darling(attributes(xdr, xdrs), default)]
pub struct VariantAttrs {
    // The name for the XDR item, if different from the rust name
    pub name: Option<String>,

    pub default_variant: bool
}

impl InputAttrs {
    pub fn crate_path(&self) -> syn::Path {
        self.krate
            .clone()
            .unwrap_or_else(|| syn::parse_quote!(::xdrs))
    }

    pub fn check<S: syn::spanned::Spanned>(&self, item: &S) -> Result<()> {
        if self.via.is_some() {
            if self.into_type.is_some() {
                err(
                    item,
                    "Attributes `xdr(via)` and `xdr(into_type)` may not be combined, \
                    as `via` implies both `from_type` and `into_type`.",
                )?;
            }

            if self.from_type.is_some() {
                err(
                    item,
                    "Attributes `xdr(via)` and `xdr(from_type)` may not be combined, \
                    as `via` implies both `from_type` and `into_type`.",
                )?;
            }
        }

        Ok(())
    }
}

impl FieldAttrs {
    pub fn check<S: syn::spanned::Spanned>(&self, item: &S) -> Result<()> {
        if self.via.is_some() {
            if self.into_type.is_some() {
                err(
                    item,
                    "Attributes `xdr(via)` and `xdr(into_type)` may not be combined, \
                    as `via` implies both `from_type` and `into_type`.",
                )?;
            }

            if self.from_type.is_some() {
                err(
                    item,
                    "Attributes `xdr(via)` and `xdr(from_type)` may not be combined, \
                    as `via` implies both `from_type` and `into_type`.",
                )?;
            }
        }

        Ok(())
    }
}
