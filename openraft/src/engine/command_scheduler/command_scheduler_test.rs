use super::CommandScheduler;
use crate::batch::Batch;
use crate::config::Config;
use crate::engine::Command;
use crate::engine::engine_output::EngineOutput;
use crate::engine::testing::UTConfig;
use crate::engine::testing::UTLeaderId;
use crate::impls::Vote;
use crate::testing::blank_ent;
use crate::vote::raft_vote::RaftVoteExt;

type C = UTConfig;

fn committed_vote(term: u64, node_id: u64) -> crate::vote::committed::CommittedVote<UTLeaderId> {
    Vote::<UTLeaderId>::new(term, node_id).to_committed()
}

fn config_with_max_append(max_append_entries: u64) -> Config {
    Config {
        max_append_entries: Some(max_append_entries),
        ..Default::default()
    }
}

#[test]
fn test_merge_empty_queue() {
    let config = Config::default();
    let mut output: EngineOutput<C> = EngineOutput::default();
    let mut scheduler = CommandScheduler::new(&config, &mut output);
    scheduler.merge_front_append_entries();
    assert!(output.commands.is_empty());
}

#[test]
fn test_merge_single_command() {
    let config = Config::default();
    let mut output: EngineOutput<C> = EngineOutput::default();
    output.push_command(Command::AppendEntries {
        committed_vote: committed_vote(1, 0),
        entries: Batch::of([blank_ent::<UTConfig>(1, 0, 1)]),
    });
    let mut scheduler = CommandScheduler::new(&config, &mut output);
    scheduler.merge_front_append_entries();
    assert_eq!(output.commands.len(), 1);
}

#[test]
fn test_merge_front_not_append_entries() {
    let config = Config::default();
    let mut output: EngineOutput<C> = EngineOutput::default();
    output.push_command(Command::SaveVote { vote: Vote::new(1, 0) });
    output.push_command(Command::AppendEntries {
        committed_vote: committed_vote(1, 0),
        entries: Batch::of([blank_ent::<UTConfig>(1, 0, 1)]),
    });
    let mut scheduler = CommandScheduler::new(&config, &mut output);
    scheduler.merge_front_append_entries();
    assert_eq!(output.commands.len(), 2);
}

#[test]
fn test_merge_consecutive_same_vote() {
    let config = Config::default();
    let mut output: EngineOutput<C> = EngineOutput::default();
    output.push_command(Command::AppendEntries {
        committed_vote: committed_vote(1, 0),
        entries: Batch::of([blank_ent::<UTConfig>(1, 0, 1)]),
    });
    output.push_command(Command::AppendEntries {
        committed_vote: committed_vote(1, 0),
        entries: Batch::of([blank_ent::<UTConfig>(1, 0, 2)]),
    });
    output.push_command(Command::AppendEntries {
        committed_vote: committed_vote(1, 0),
        entries: Batch::of([blank_ent::<UTConfig>(1, 0, 3)]),
    });

    let mut scheduler = CommandScheduler::new(&config, &mut output);
    scheduler.merge_front_append_entries();

    assert_eq!(output.commands.len(), 1);
    if let Command::AppendEntries { entries, .. } = &output.commands[0] {
        assert_eq!(entries.len(), 3);
    } else {
        panic!("Expected AppendEntries");
    }
}

#[test]
fn test_merge_stops_at_different_vote() {
    let config = Config::default();
    let mut output: EngineOutput<C> = EngineOutput::default();
    output.push_command(Command::AppendEntries {
        committed_vote: committed_vote(1, 0),
        entries: Batch::of([blank_ent::<UTConfig>(1, 0, 1)]),
    });
    output.push_command(Command::AppendEntries {
        committed_vote: committed_vote(2, 0), // Different vote
        entries: Batch::of([blank_ent::<UTConfig>(2, 0, 2)]),
    });

    let mut scheduler = CommandScheduler::new(&config, &mut output);
    scheduler.merge_front_append_entries();

    assert_eq!(output.commands.len(), 2);
}

#[test]
fn test_merge_stops_at_non_append_entries() {
    let config = Config::default();
    let mut output: EngineOutput<C> = EngineOutput::default();
    output.push_command(Command::AppendEntries {
        committed_vote: committed_vote(1, 0),
        entries: Batch::of([blank_ent::<UTConfig>(1, 0, 1)]),
    });
    output.push_command(Command::AppendEntries {
        committed_vote: committed_vote(1, 0),
        entries: Batch::of([blank_ent::<UTConfig>(1, 0, 2)]),
    });
    output.push_command(Command::SaveVote { vote: Vote::new(1, 0) });
    output.push_command(Command::AppendEntries {
        committed_vote: committed_vote(1, 0),
        entries: Batch::of([blank_ent::<UTConfig>(1, 0, 3)]),
    });

    let mut scheduler = CommandScheduler::new(&config, &mut output);
    scheduler.merge_front_append_entries();

    // First two AppendEntries merged, SaveVote and last AppendEntries remain
    assert_eq!(output.commands.len(), 3);
    if let Command::AppendEntries { entries, .. } = &output.commands[0] {
        assert_eq!(entries.len(), 2);
    } else {
        panic!("Expected AppendEntries");
    }
}

#[test]
fn test_merge_stops_at_max_entries() {
    // Limit to 3 entries: first command has 2, second has 1, third would exceed
    let config = config_with_max_append(3);
    let mut output: EngineOutput<C> = EngineOutput::default();
    output.push_command(Command::AppendEntries {
        committed_vote: committed_vote(1, 0),
        entries: Batch::of([blank_ent::<UTConfig>(1, 0, 1), blank_ent::<UTConfig>(1, 0, 2)]),
    });
    output.push_command(Command::AppendEntries {
        committed_vote: committed_vote(1, 0),
        entries: Batch::of([blank_ent::<UTConfig>(1, 0, 3)]),
    });
    output.push_command(Command::AppendEntries {
        committed_vote: committed_vote(1, 0),
        entries: Batch::of([blank_ent::<UTConfig>(1, 0, 4)]),
    });

    let mut scheduler = CommandScheduler::new(&config, &mut output);
    scheduler.merge_front_append_entries();

    // First two merged (3 entries), third remains separate
    assert_eq!(output.commands.len(), 2);
    if let Command::AppendEntries { entries, .. } = &output.commands[0] {
        assert_eq!(entries.len(), 3);
    } else {
        panic!("Expected AppendEntries");
    }
}
