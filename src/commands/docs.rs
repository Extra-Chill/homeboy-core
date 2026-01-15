use clap::Args;

use crate::docs;

use super::CmdResult;

#[derive(Args)]
pub struct DocsArgs {
    /// Topic path (e.g., 'commands/deploy') or 'list' to show available topics
    #[arg(trailing_var_arg = true)]
    pub topic: Vec<String>,
}

pub fn run(args: DocsArgs) -> CmdResult<String> {
    if args.topic.len() == 1 && args.topic[0] == "list" {
        let topics = docs::available_topics();
        return Ok((topics.join("\n"), 0));
    }

    let resolved = docs::resolve(&args.topic)?;
    Ok((resolved.content, 0))
}
