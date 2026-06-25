use crate::attr::ContainerAttrs;

use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::quote;
use syn::{Error, Result as SynResult};

/// How an enum maps onto the tree (Serde-style tagging); `External` is the default.
pub(super) enum EnumRepr {
    /// `{ Variant: payload }` — the object's single key is the tag.
    External,
    /// `{ <tag>: "Variant", <content>: payload }` — tag and payload in named fields.
    Adjacent { tag: String, content: String },
    /// `{ <tag>: "Variant", ..payload }` — tag and payload flattened into one object;
    /// tuple/newtype elements are keyed by their decimal index (`"0"`, `"1"`, …).
    Internal { tag: String },
}

impl EnumRepr {
    pub(super) fn from_container(container: &ContainerAttrs, name: &Ident) -> SynResult<Self> {
        match (container.tag(), container.content(), container.untagged()) {
            (None, None, false) => Ok(Self::External),
            (Some(tag), Some(content), false) => Ok(Self::Adjacent {
                tag:     tag.to_string(),
                content: content.to_string(),
            }),
            (Some(tag), None, false) => Ok(Self::Internal {
                tag: tag.to_string()
            }),
            (None, Some(_), false) => Err(Error::new(name.span(), "`content` requires `tag`")),
            (_, _, true) => Err(Error::new(name.span(), "untagged enums are not supported yet")),
        }
    }

    /// Store statement writing the variant tag — empty for `External`, where the
    /// tag IS the object key created by writing the payload.
    pub(super) fn tag_store(&self, variant_tag: &str) -> TokenStream2 {
        match self {
            Self::External => quote! {},
            Self::Adjacent {
                tag, ..
            }
            | Self::Internal {
                tag,
            } => quote! {
                ::stratodb::data::SData::store(
                    &::std::string::String::from(#variant_tag),
                    writer,
                    &at.child_name(#tag),
                )?;
            },
        }
    }

    /// Store statement(s) for a unit variant's body (the `… => { HERE }` of the arm).
    pub(super) fn unit_store(&self, variant_tag: &str) -> TokenStream2 {
        match self {
            // External: the tag IS the object key, carrying a `Null` value.
            Self::External => quote! {
                ::stratodb::access::Writer::put_scalar(
                    writer,
                    &at.child_name(#variant_tag),
                    ::stratodb::data::Scalar::Null,
                )?;
            },
            // Tagged: just the tag field (a unit variant has no payload).
            Self::Adjacent {
                ..
            }
            | Self::Internal {
                ..
            } => self.tag_store(variant_tag),
        }
    }

    /// The base path a non-unit variant's payload is written under. External and
    /// adjacent only — internal flattens the payload and is handled separately.
    pub(super) fn payload_base_store(&self, variant_tag: &str) -> TokenStream2 {
        match self {
            Self::External => quote! { at.child_name(#variant_tag) },
            Self::Adjacent {
                content, ..
            } => quote! { at.child_name(#content) },
            Self::Internal {
                ..
            } => unreachable!("internal tagging is handled by internal_store_arm"),
        }
    }

    /// Statements binding `tag: String` ahead of the load match.
    pub(super) fn tag_load(&self) -> TokenStream2 {
        match self {
            Self::External => quote! {
                let key = ::stratodb::access::Reader::resolve(reader, at)?
                    .ok_or_else(|| ::stratodb::SdbError::PathNotFound(at.clone()))?;

                let tag = ::stratodb::access::Reader::object_keys(reader, key)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| {
                        ::stratodb::SdbError::Corrupt(::std::string::String::from("enum node has no variant tag"))
                    })?;
            },
            Self::Adjacent {
                tag, ..
            }
            | Self::Internal {
                tag,
            } => quote! {
                let tag = <::std::string::String as ::stratodb::data::SData>::load(reader, &at.child_name(#tag))?;
            },
        }
    }

    /// The base path a non-unit variant's payload is read from. External and
    /// adjacent only — internal flattens the payload and is handled separately.
    pub(super) fn payload_base_load(&self) -> TokenStream2 {
        match self {
            Self::External => quote! { at.child_name(tag.as_str()) },
            Self::Adjacent {
                content, ..
            } => quote! { at.child_name(#content) },
            Self::Internal {
                ..
            } => unreachable!("internal tagging is handled by internal_load_arm"),
        }
    }

    /// The body of the accessor `variant()`; `handle` is `self.reader`/`self.writer`.
    pub(super) fn variant_body(&self, handle: TokenStream2) -> TokenStream2 {
        match self {
            Self::External => quote! {
                ::stratodb::access::Reader::object_keys(&#handle, self.key)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| {
                        ::stratodb::SdbError::Corrupt(::std::string::String::from("enum node has no variant tag"))
                    })
            },
            Self::Adjacent {
                tag, ..
            }
            | Self::Internal {
                tag,
            } => quote! {
                <::std::string::String as ::stratodb::data::SData>::load(&#handle, &self.base.child_name(#tag))
            },
        }
    }
}
