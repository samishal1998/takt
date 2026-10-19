use super::*;

fn process(pid: i32, parent: i32, group: i32, started: u64) -> Process {
    Process {
        identity: Identity {
            pid,
            started: (started, 0),
        },
        parent,
        group,
        live: true,
        #[cfg(target_os = "linux")]
        resident_bytes: Some(0),
    }
}

fn table(processes: impl IntoIterator<Item = Process>) -> ProcessTable {
    let processes: BTreeMap<_, _> = processes
        .into_iter()
        .map(|process| (process.identity.pid, process))
        .collect();
    let mut children: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    for (&pid, process) in &processes {
        children.entry(process.parent).or_default().push(pid);
    }
    ProcessTable {
        processes,
        children,
        unreadable: BTreeSet::new(),
    }
}

fn tracked_ids(tree: &ProcessTree) -> Vec<i32> {
    tree.tracked.keys().copied().collect()
}

#[test]
fn changed_groups_do_not_break_multilevel_descendant_ownership() {
    let snapshot = table([
        process(100, 1, 100, 1),
        process(200, 100, 200, 2),
        process(300, 200, 300, 3),
        process(400, 300, 400, 4),
        process(500, 1, 500, 5),
        process(600, 500, 600, 6),
        process(700, 1, 100, 7),
    ]);
    let mut tree = ProcessTree::default();
    tree.observe(&snapshot, 100, 100);
    assert_eq!(tracked_ids(&tree), [100, 200, 300, 400, 700]);
}

#[test]
fn observed_descendants_stay_owned_after_reparenting_and_find_new_children() {
    let mut tree = ProcessTree::default();
    tree.observe(
        &table([process(100, 1, 100, 1), process(200, 100, 200, 2)]),
        100,
        100,
    );
    tree.tracked.get_mut(&200).unwrap().signalled = Some(libc::SIGTERM);
    tree.observe(
        &table([
            process(100, 1, 100, 1),
            process(200, 1, 900, 2),
            process(300, 200, 300, 3),
        ]),
        100,
        100,
    );
    assert_eq!(tracked_ids(&tree), [100, 200, 300]);
    assert_eq!(tree.tracked[&200].signalled, Some(libc::SIGTERM));
}

#[test]
fn reused_descendant_pid_does_not_transfer_ownership_to_its_new_subtree() {
    let mut tree = ProcessTree::default();
    tree.observe(
        &table([
            process(100, 1, 100, 1),
            process(200, 100, 200, 2),
            process(300, 200, 300, 3),
        ]),
        100,
        100,
    );
    tree.observe(
        &table([
            process(100, 1, 100, 1),
            process(200, 1, 900, 20),
            process(300, 1, 300, 3),
            process(400, 200, 400, 21),
        ]),
        100,
        100,
    );
    assert_eq!(tracked_ids(&tree), [100, 300]);
    assert_eq!(tree.tracked[&300].identity.started, (3, 0));
}

#[test]
fn unreadable_tracked_identity_is_retained_until_a_complete_snapshot_loses_it() {
    let leader = process(100, 1, 100, 1);
    let mut tree = ProcessTree::default();
    tree.observe(&table([leader, process(200, 100, 200, 2)]), 100, 100);
    let mut inaccessible = table([leader]);
    inaccessible.unreadable.extend([200, 999]);
    tree.observe(&inaccessible, 100, 100);
    assert_eq!(tracked_ids(&tree), [100, 200]);
    #[cfg(target_os = "linux")]
    assert_eq!(tree.memory(&inaccessible, 100, 100), None);
    tree.observe(&table([leader]), 100, 100);
    assert_eq!(tracked_ids(&tree), [100]);
}

#[cfg(target_os = "linux")]
#[test]
fn memory_ignores_zombies_but_not_unknown_live_descendants() {
    let mut leader = process(100, 1, 100, 1);
    leader.live = false;
    leader.resident_bytes = Some(1_000);
    let mut child = process(200, 100, 200, 2);
    child.resident_bytes = Some(42);
    let mut zombie = process(300, 200, 300, 3);
    zombie.live = false;
    zombie.resident_bytes = None;
    let mut tree = ProcessTree::default();
    assert_eq!(
        tree.memory(&table([leader, child, zombie]), 100, 100),
        Some(42)
    );
    child.resident_bytes = None;
    assert_eq!(tree.memory(&table([leader, child, zombie]), 100, 100), None);
}

#[cfg(target_os = "linux")]
#[test]
fn resident_sum_overflow_is_unknown_instead_of_wrapping() {
    let mut leader = process(100, 1, 100, 1);
    leader.resident_bytes = Some(u64::MAX);
    let mut child = process(200, 100, 200, 2);
    child.resident_bytes = Some(1);
    assert_eq!(
        ProcessTree::default().memory(&table([leader, child]), 100, 100),
        None
    );
}

#[cfg(target_os = "linux")]
#[test]
fn old_ppid_edge_does_not_attach_a_process_that_predates_the_current_parent() {
    let snapshot = table([
        process(100, 1, 100, 10),
        process(200, 100, 200, 2),
        process(300, 200, 300, 3),
    ]);
    let mut tree = ProcessTree::default();
    tree.observe(&snapshot, 100, 100);
    assert_eq!(tracked_ids(&tree), [100]);
}

#[cfg(target_os = "linux")]
fn proc_stat_with_rss(rss: &str) -> String {
    let mut fields = ["0"; 22]; // fields 3 through 24
    fields[0] = "S";
    fields[1] = "7";
    fields[2] = "8";
    fields[19] = "12345678"; // start time, field 22
    fields[20] = "98765"; // virtual bytes, field 23
    fields[21] = rss;
    format!("42 (worker (phase one)\nwith spaces) {}", fields.join(" "))
}

#[cfg(target_os = "linux")]
#[test]
fn proc_stat_parses_identity_and_rss_after_parentheses_spaces_and_newlines_in_comm() {
    let process = parse_proc_stat(42, &proc_stat_with_rss("9"), 4096).unwrap();
    assert_eq!(
        process.identity,
        Identity {
            pid: 42,
            started: (12345678, 0),
        }
    );
    assert_eq!(process.parent, 7);
    assert_eq!(process.group, 8);
    assert!(process.live);
    assert_eq!(process.resident_bytes, Some(9 * 4096));
}

#[cfg(target_os = "linux")]
#[test]
fn proc_stat_keeps_invalid_rss_or_page_size_unknown() {
    for (rss, page_size) in [
        ("-1", 4096),
        ("9", 0),
        ("9", -1),
        ("9223372036854775807", 4096),
    ] {
        let process = parse_proc_stat(42, &proc_stat_with_rss(rss), page_size).unwrap();
        assert_eq!(
            process.resident_bytes, None,
            "rss={rss} page_size={page_size}"
        );
        assert_eq!(process.identity.started, (12345678, 0));
    }
}
