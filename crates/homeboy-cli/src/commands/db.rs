use clap::{Args, Subcommand};
use serde::Serialize;

use homeboy::db::{self, DbResult, DbTunnelResult};
use homeboy::project;
use homeboy::token;

#[derive(Args)]
pub struct DbArgs {
    #[command(subcommand)]
    command: DbCommand,
}

#[derive(Subcommand)]
enum DbCommand {
    /// List database tables
    Tables {
        /// Project ID
        project_id: String,
        /// Optional subtarget
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Show table structure
    Describe {
        /// Project ID
        project_id: String,
        /// Optional subtarget and table name
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Execute SELECT query
    Query {
        /// Project ID
        project_id: String,
        /// Optional subtarget and SQL query
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Delete a row from a table
    DeleteRow {
        /// Project ID
        project_id: String,
        /// Table name and row ID
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
        /// Confirm destructive operation
        #[arg(long)]
        confirm: bool,
    },
    /// Drop a database table
    DropTable {
        /// Project ID
        project_id: String,
        /// Table name
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
        /// Confirm destructive operation
        #[arg(long)]
        confirm: bool,
    },
    /// Open SSH tunnel to database
    Tunnel {
        /// Project ID
        project_id: String,
        /// Local port to bind
        #[arg(long)]
        local_port: Option<u16>,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbOutput {
    pub command: String,
    #[serde(flatten)]
    pub result: DbResultVariant,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum DbResultVariant {
    Query(DbResult),
    Tunnel(DbTunnelResult),
}

pub fn run(
    args: DbArgs,
    _global: &crate::commands::GlobalArgs,
) -> homeboy::Result<(DbOutput, i32)> {
    match args.command {
        DbCommand::Tables { project_id, args } => tables(&project_id, &args),
        DbCommand::Describe { project_id, args } => describe(&project_id, &args),
        DbCommand::Query { project_id, args } => query(&project_id, &args),
        DbCommand::DeleteRow {
            project_id,
            args,
            confirm,
        } => delete_row(&project_id, &args, confirm),
        DbCommand::DropTable {
            project_id,
            args,
            confirm,
        } => drop_table(&project_id, &args, confirm),
        DbCommand::Tunnel {
            project_id,
            local_port,
        } => tunnel(&project_id, local_port),
    }
}

fn parse_subtarget(project_id: &str, args: &[String]) -> homeboy::Result<(Option<String>, Vec<String>)> {
    let project = project::load_record(project_id)?;

    if project.config.sub_targets.is_empty() {
        return Ok((None, args.to_vec()));
    }

    if let Some(sub_id) = args.first() {
        if project.config.sub_targets.iter().any(|target| {
            project::slugify_id(&target.name).ok().as_deref() == Some(sub_id)
                || token::identifier_eq(&target.name, sub_id)
        }) {
            return Ok((Some(sub_id.clone()), args[1..].to_vec()));
        }
    }

    Ok((None, args.to_vec()))
}

fn tables(project_id: &str, args: &[String]) -> homeboy::Result<(DbOutput, i32)> {
    let (subtarget, _) = parse_subtarget(project_id, args)?;
    let result = db::list_tables(project_id, subtarget.as_deref())?;
    let exit_code = result.exit_code;

    Ok((
        DbOutput {
            command: "db.tables".to_string(),
            result: DbResultVariant::Query(result),
        },
        exit_code,
    ))
}

fn describe(project_id: &str, args: &[String]) -> homeboy::Result<(DbOutput, i32)> {
    let (subtarget, remaining) = parse_subtarget(project_id, args)?;

    let table_name = remaining
        .first()
        .ok_or_else(|| homeboy::Error::config("Table name required".to_string()))?;

    let result = db::describe_table(project_id, table_name, subtarget.as_deref())?;
    let exit_code = result.exit_code;

    Ok((
        DbOutput {
            command: "db.describe".to_string(),
            result: DbResultVariant::Query(result),
        },
        exit_code,
    ))
}

fn query(project_id: &str, args: &[String]) -> homeboy::Result<(DbOutput, i32)> {
    let (subtarget, remaining) = parse_subtarget(project_id, args)?;
    let sql = remaining.join(" ");

    let result = db::query(project_id, &sql, subtarget.as_deref())?;
    let exit_code = result.exit_code;

    Ok((
        DbOutput {
            command: "db.query".to_string(),
            result: DbResultVariant::Query(result),
        },
        exit_code,
    ))
}

fn delete_row(
    project_id: &str,
    args: &[String],
    confirm: bool,
) -> homeboy::Result<(DbOutput, i32)> {
    if !confirm {
        return Err(homeboy::Error::config(
            "Use --confirm to execute destructive operations".to_string(),
        ));
    }

    let (subtarget, remaining) = parse_subtarget(project_id, args)?;

    if remaining.len() < 2 {
        return Err(homeboy::Error::config(
            "Table name and row ID required".to_string(),
        ));
    }

    let table_name = &remaining[0];
    let row_id: i64 = remaining[1]
        .parse()
        .map_err(|_| homeboy::Error::config("Row ID must be numeric".to_string()))?;

    let result = db::delete_row(project_id, table_name, row_id, subtarget.as_deref())?;
    let exit_code = result.exit_code;

    Ok((
        DbOutput {
            command: "db.deleteRow".to_string(),
            result: DbResultVariant::Query(result),
        },
        exit_code,
    ))
}

fn drop_table(
    project_id: &str,
    args: &[String],
    confirm: bool,
) -> homeboy::Result<(DbOutput, i32)> {
    if !confirm {
        return Err(homeboy::Error::config(
            "Use --confirm to execute destructive operations".to_string(),
        ));
    }

    let (subtarget, remaining) = parse_subtarget(project_id, args)?;

    let table_name = remaining
        .first()
        .ok_or_else(|| homeboy::Error::config("Table name required".to_string()))?;

    let result = db::drop_table(project_id, table_name, subtarget.as_deref())?;
    let exit_code = result.exit_code;

    Ok((
        DbOutput {
            command: "db.dropTable".to_string(),
            result: DbResultVariant::Query(result),
        },
        exit_code,
    ))
}

fn tunnel(project_id: &str, local_port: Option<u16>) -> homeboy::Result<(DbOutput, i32)> {
    let result = db::create_tunnel(project_id, local_port)?;
    let exit_code = result.exit_code;

    Ok((
        DbOutput {
            command: "db.tunnel".to_string(),
            result: DbResultVariant::Tunnel(result),
        },
        exit_code,
    ))
}
