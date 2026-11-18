use std::ffi::OsString;

use clap::{Arg, ArgAction, Command, value_parser};

/// The command `units::dispatch` gets when argv names no subcommand.
pub const ROOT: &str = "fsearch";

fn rest() -> Arg {
    Arg::new("args")
        .num_args(0..)
        .trailing_var_arg(true)
        .allow_hyphen_values(true)
        .value_parser(value_parser!(OsString))
}

/// Parses argv into a command and its arguments.
pub fn command() -> Command {
    Command::new(ROOT)
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true)
        .args_conflicts_with_subcommands(true)
        .arg(rest())
        .subcommand(Command::new("dump").disable_help_flag(true).arg(rest()))
        .subcommand(
            Command::new("query")
                .disable_help_flag(true)
                .arg(Arg::new("resort").long("resort").action(ArgAction::Count))
                .arg(rest()),
        )
        .subcommand(Command::new("resave").disable_help_flag(true).arg(rest()))
        .subcommand(
            Command::new("scandump")
                .disable_help_flag(true)
                .arg(Arg::new("exclude").long("exclude").action(ArgAction::Count))
                .arg(
                    Arg::new("exclude-hidden")
                        .long("exclude-hidden")
                        .action(ArgAction::Count),
                )
                .arg(rest()),
        )
        .subcommand(
            Command::new("scanmd5")
                .disable_help_flag(true)
                .arg(Arg::new("exclude").long("exclude").action(ArgAction::Count))
                .arg(
                    Arg::new("exclude-hidden")
                        .long("exclude-hidden")
                        .action(ArgAction::Count),
                )
                .arg(rest()),
        )
}
