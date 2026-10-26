mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

#[test]
fn scratch_is_removed_after_success_and_unwind_without_touching_a_sibling() {
    // The parent guard makes the test's own control clean up even if an assertion below fails.
    let parent = common::Scratch::new("scratch-cleanup", "control");
    let sibling = parent.join("unrelated");
    std::fs::create_dir_all(&sibling).expect("unrelated control");
    std::fs::write(sibling.join("keep"), b"unrelated").expect("unrelated marker");

    let success = common::Scratch::new("scratch-cleanup", "success");
    let success_path = success.0.clone();
    std::fs::write(success.join("payload"), b"temporary").expect("success payload");
    drop(success);
    assert!(
        !success_path.exists(),
        "a successful scope removes its scratch"
    );
    assert!(
        sibling.join("keep").is_file(),
        "a sibling is not cleanup input"
    );

    let unwind_path = std::env::temp_dir().join(format!(
        "sakatsu-scratch-cleanup-unwind-{}",
        std::process::id()
    ));
    let unwind = catch_unwind(AssertUnwindSafe(|| {
        let scratch = common::Scratch::new("scratch-cleanup", "unwind");
        std::fs::write(scratch.join("payload"), b"temporary").expect("unwind payload");
        panic!("exercise Drop during unwinding");
    }));
    assert!(unwind.is_err(), "the control must actually unwind");
    assert!(
        !unwind_path.exists(),
        "an assertion failure must not leave the scratch behind"
    );
    assert!(
        sibling.join("keep").is_file(),
        "unwind cleanup must remain scoped to its own directory"
    );
}
