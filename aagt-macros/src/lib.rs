//! # AAGT Macros
//!
//! Procedural macros for the AAGT AI Agent framework.
//!
//! (Updated for AAGT v0.1.2)
//!
//! ## `#[tool]` Macro
//!
//! Automatically implements the `Tool` trait for a struct.
//!
//! ### Example
//!
//! ```ignore
//! use aagt_macros::tool;
//! use aagt_core::prelude::*;
//!
//! #[tool(
//!     name = "get_token_price",
//!     description = "Get the current price of a cryptocurrency token"
//! )]
//! struct GetTokenPrice;
//!
//! #[derive(serde::Deserialize)]
//! struct GetTokenPriceArgs {
//!     /// Token symbol (e.g., SOL, ETH)
//!     symbol: String,
//! }
//!
//! impl GetTokenPrice {
//!     async fn execute(&self, args: GetTokenPriceArgs) -> Result<String> {
//!         Ok(format!("{} price: $185.50", args.symbol))
//!     }
//! }
//! ```

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{parse_macro_input, DeriveInput, Ident, LitStr, Token};

/// Arguments for the `#[tool]` attribute
struct ToolArgs {
    name: String,
    description: String,
    args_type: Option<String>,
}

impl Parse for ToolArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut name = None;
        let mut description = None;
        let mut args_type = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "name" => {
                    let value: LitStr = input.parse()?;
                    name = Some(value.value());
                }
                "description" => {
                    let value: LitStr = input.parse()?;
                    description = Some(value.value());
                }
                "args" => {
                    let value: Ident = input.parse()?;
                    args_type = Some(value.to_string());
                }
                _ => {
                    return Err(syn::Error::new(key.span(), "unknown attribute"));
                }
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(ToolArgs {
            name: name.ok_or_else(|| syn::Error::new(input.span(), "missing 'name'"))?,
            description: description
                .ok_or_else(|| syn::Error::new(input.span(), "missing 'description'"))?,
            args_type,
        })
    }
}

/// Derive macro for implementing the `Tool` trait.
///
/// # Arguments
///
/// * `name` - The tool name (used by LLM)
/// * `description` - Description for the LLM
/// * `args` - (Optional) The arguments struct type name
///
/// # Example
///
/// ```ignore
/// #[tool(name = "swap_tokens", description = "Swap cryptocurrency tokens")]
/// struct SwapTokens {
///     // ... fields
/// }
/// ```
#[proc_macro_attribute]
pub fn tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as ToolArgs);
    let input = parse_macro_input!(item as DeriveInput);

    let struct_name = &input.ident;
    let tool_name = &args.name;
    let tool_description = &args.description;

    // Default args type is StructNameArgs
    let args_type_name = args
        .args_type
        .unwrap_or_else(|| format!("{}Args", struct_name));
    let args_type = format_ident!("{}", args_type_name);

    let expanded = quote! {
        #input

        #[async_trait::async_trait]
        impl aagt_core::tool::Tool for #struct_name {
            fn name(&self) -> &str {
                #tool_name
            }

            fn definition(&self) -> aagt_core::tool::ToolDefinition {
                let gen = schemars::gen::SchemaSettings::openapi3().into_generator();
                let schema = gen.into_root_schema_for::<#args_type>();
                let schema_json = serde_json::to_value(schema).unwrap_or(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }));

                aagt_core::tool::ToolDefinition {
                    name: #tool_name.to_string(),
                    description: #tool_description.to_string(),
                    parameters: schema_json,
                    parameters_ts: None, // TODO: Implement TS generation from schema
                }
            }

            async fn call(&self, arguments: &str) -> aagt_core::anyhow::Result<String> {
                let args: #args_type = serde_json::from_str(arguments)
                    .map_err(|e| aagt_core::error::Error::ToolArguments {
                        tool_name: #tool_name.to_string(),
                        message: e.to_string(),
                    })?;

                self.execute(args).await
                    .map_err(|e| e.into())
            }
        }
    };

    TokenStream::from(expanded)
}

/// Derive macro for generating Tool implementation with JSON schema.
///
/// This requires the `schemars` crate for JSON schema generation.
///
/// # Example
///
/// ```ignore
/// use schemars::JsonSchema;
///
/// #[derive(Tool)]
/// #[tool(name = "get_price", description = "Get token price")]
/// struct GetPrice;
///
/// #[derive(Deserialize, JsonSchema)]
/// struct GetPriceArgs {
///     /// The token symbol
///     symbol: String,
/// }
/// ```
#[proc_macro_derive(Tool, attributes(tool))]
pub fn derive_tool(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;

    // Parse attributes to find tool(name = "...", description = "...")
    let mut tool_name = None;
    let mut tool_description = None;

    for attr in &input.attrs {
        if attr.path().is_ident("tool") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    let value: LitStr = meta.value()?.parse()?;
                    tool_name = Some(value.value());
                } else if meta.path.is_ident("description") {
                    let value: LitStr = meta.value()?.parse()?;
                    tool_description = Some(value.value());
                }
                Ok(())
            });
        }
    }

    let name = tool_name.unwrap_or_else(|| struct_name.to_string().to_lowercase());
    let description = tool_description.unwrap_or_else(|| format!("Tool: {}", struct_name));
    let args_type = format_ident!("{}Args", struct_name);

    let expanded = quote! {
        #[async_trait::async_trait]
        impl aagt_core::tool::Tool for #struct_name {
            fn name(&self) -> &str {
                #name
            }

            fn definition(&self) -> aagt_core::tool::ToolDefinition {
                let gen = schemars::gen::SchemaSettings::openapi3().into_generator();
                let schema = gen.into_root_schema_for::<#args_type>();
                let schema_json = serde_json::to_value(schema).unwrap_or(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }));

                aagt_core::tool::ToolDefinition {
                    name: #name.to_string(),
                    description: #description.to_string(),
                    parameters: schema_json,
                    parameters_ts: None,
                }
            }

            async fn call(&self, arguments: &str) -> aagt_core::anyhow::Result<String> {
                let args: #args_type = serde_json::from_str(arguments)
                    .map_err(|e| aagt_core::error::Error::ToolArguments {
                        tool_name: #name.to_string(),
                        message: e.to_string(),
                    })?;

                self.execute(args).await
                    .map_err(|e| e.into())
            }
        }
    };

    TokenStream::from(expanded)
}
