use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use colored::Colorize;

use crate::api::{ApiClient, InvoiceListParams};
use crate::cli::commands::org::resolve_org_id;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum InvoiceSortBy {
    CreatedAt,
    IssuedAt,
    DueDate,
}

impl InvoiceSortBy {
    fn as_str(self) -> &'static str {
        match self {
            InvoiceSortBy::CreatedAt => "created_at",
            InvoiceSortBy::IssuedAt => "issued_at",
            InvoiceSortBy::DueDate => "due_date",
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    fn as_str(self) -> &'static str {
        match self {
            SortOrder::Asc => "asc",
            SortOrder::Desc => "desc",
        }
    }
}

#[derive(Subcommand)]
pub enum InvoiceCommands {
    /// List invoices for an organization
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Max number of invoices to return (default 12)
        #[arg(long)]
        limit: Option<u16>,
        /// Pagination offset (default 0)
        #[arg(long)]
        offset: Option<u16>,
        /// Sort field (default due-date)
        #[arg(long, value_enum)]
        sort_by: Option<InvoiceSortBy>,
        /// Sort order (default asc)
        #[arg(long, value_enum)]
        sort_order: Option<SortOrder>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Build a client whose `X-Organization-ID` header targets `org_id`.
/// Mirrors `member::client_for_org` — invoices are resolved purely from the
/// header (no organization id in the URL path).
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

pub async fn handle_invoice(cmd: &InvoiceCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        InvoiceCommands::List {
            org,
            limit,
            offset,
            sort_by,
            sort_order,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let list = scoped
                .list_invoices(InvoiceListParams {
                    limit: *limit,
                    offset: *offset,
                    sort_by: sort_by.map(InvoiceSortBy::as_str),
                    sort_order: sort_order.map(SortOrder::as_str),
                })
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else if list.data.is_empty() {
                println!("{}", "No invoices found.".dimmed());
            } else {
                println!(
                    "{:<38} {:<12} {:>10} {:<6} {:<12} {}",
                    "ID".bold(),
                    "STATUS".bold(),
                    "AMOUNT".bold(),
                    "CCY".bold(),
                    "DUE".bold(),
                    "TYPE".bold()
                );
                println!("{}", "─".repeat(100));
                for invoice in &list.data {
                    println!(
                        "{:<38} {:<12} {:>10} {:<6} {:<12} {}",
                        invoice.id.dimmed(),
                        invoice.status,
                        invoice.amount,
                        invoice.currency,
                        invoice.due_date.as_deref().unwrap_or("-"),
                        invoice.invoice_type
                    );
                }
                println!();
                println!(
                    "Total: {} (showing {})",
                    list.pagination.total,
                    list.data.len()
                );
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{InvoiceSortBy, SortOrder};

    #[test]
    fn invoice_sort_by_as_str_maps_to_backend_values() {
        assert_eq!(InvoiceSortBy::CreatedAt.as_str(), "created_at");
        assert_eq!(InvoiceSortBy::IssuedAt.as_str(), "issued_at");
        assert_eq!(InvoiceSortBy::DueDate.as_str(), "due_date");
    }

    #[test]
    fn sort_order_as_str_maps_to_backend_values() {
        assert_eq!(SortOrder::Asc.as_str(), "asc");
        assert_eq!(SortOrder::Desc.as_str(), "desc");
    }
}
