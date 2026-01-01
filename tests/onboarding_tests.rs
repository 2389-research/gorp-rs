// ABOUTME: Tests for the onboarding state machine
// ABOUTME: Verifies user isolation, state transitions, and edge cases

use gorp::onboarding::{OnboardingState, OnboardingStep};
use gorp::session::SessionStore;

fn create_test_store() -> (SessionStore, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let template_dir = temp_dir.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();
    let store = SessionStore::new(temp_dir.path()).unwrap();
    (store, temp_dir)
}

// =============================================================================
// SCENARIO: New user should go through onboarding
// =============================================================================

#[test]
fn scenario_new_user_needs_onboarding() {
    let (store, _temp) = create_test_store();
    let user_id = "@alice:example.com";

    // New user with no onboarding state should need onboarding
    let needs_onboarding = gorp::onboarding::should_onboard(&store, user_id).unwrap();
    assert!(needs_onboarding, "New user should need onboarding");
}

#[test]
fn scenario_user_with_completed_onboarding_skipped() {
    let (store, _temp) = create_test_store();
    let user_id = "@bob:example.com";

    // Save completed onboarding state
    let state = OnboardingState {
        step: OnboardingStep::Completed,
        started_at: "2024-01-01T00:00:00Z".to_string(),
    };
    gorp::onboarding::save_state(&store, user_id, &state).unwrap();

    // User with completed onboarding should NOT need onboarding
    let needs_onboarding = gorp::onboarding::should_onboard(&store, user_id).unwrap();
    assert!(
        !needs_onboarding,
        "Completed user should not need onboarding"
    );
}

#[test]
fn scenario_user_with_in_progress_onboarding_continues() {
    let (store, _temp) = create_test_store();
    let user_id = "@charlie:example.com";

    // Save in-progress onboarding state (at CreateChannel step)
    let state = OnboardingState {
        step: OnboardingStep::CreateChannel,
        started_at: "2024-01-01T00:00:00Z".to_string(),
    };
    gorp::onboarding::save_state(&store, user_id, &state).unwrap();

    // User with in-progress onboarding should continue
    let needs_onboarding = gorp::onboarding::should_onboard(&store, user_id).unwrap();
    assert!(
        needs_onboarding,
        "In-progress user should continue onboarding"
    );
}

// =============================================================================
// SCENARIO: User isolation - different users have independent state
// =============================================================================

#[test]
fn scenario_user_isolation_independent_states() {
    let (store, _temp) = create_test_store();
    let alice = "@alice:example.com";
    let bob = "@bob:example.com";

    // Alice completes onboarding
    let alice_state = OnboardingState {
        step: OnboardingStep::Completed,
        started_at: "2024-01-01T00:00:00Z".to_string(),
    };
    gorp::onboarding::save_state(&store, alice, &alice_state).unwrap();

    // Bob is new - should still need onboarding
    let bob_needs = gorp::onboarding::should_onboard(&store, bob).unwrap();
    assert!(
        bob_needs,
        "Bob should need onboarding even though Alice completed"
    );

    // Alice should not need onboarding
    let alice_needs = gorp::onboarding::should_onboard(&store, alice).unwrap();
    assert!(!alice_needs, "Alice should not need onboarding");
}

#[test]
fn scenario_user_isolation_channels_dont_affect_other_users() {
    let (store, _temp) = create_test_store();
    let alice = "@alice:example.com";
    let bob = "@bob:example.com";

    // Alice creates a channel (simulating she completed onboarding)
    store
        .create_channel("alice-channel", "!alice:example.com")
        .unwrap();

    // Alice marks onboarding complete
    let alice_state = OnboardingState {
        step: OnboardingStep::Completed,
        started_at: "2024-01-01T00:00:00Z".to_string(),
    };
    gorp::onboarding::save_state(&store, alice, &alice_state).unwrap();

    // Bob should still need onboarding - channels are shared but onboarding is per-user
    let bob_needs = gorp::onboarding::should_onboard(&store, bob).unwrap();
    assert!(
        bob_needs,
        "Bob should need onboarding even with existing channels"
    );
}

// =============================================================================
// SCENARIO: State transitions through onboarding flow
// =============================================================================

#[test]
fn scenario_state_transitions_welcome_to_create_channel() {
    let (store, _temp) = create_test_store();
    let user_id = "@dave:example.com";

    // Start at Welcome
    let state1 = OnboardingState {
        step: OnboardingStep::Welcome,
        started_at: "2024-01-01T00:00:00Z".to_string(),
    };
    gorp::onboarding::save_state(&store, user_id, &state1).unwrap();

    // Verify we're at Welcome
    let loaded = gorp::onboarding::get_state(&store, user_id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.step, OnboardingStep::Welcome);

    // Should not be waiting for channel name yet
    let waiting = gorp::onboarding::is_waiting_for_channel_name(&store, user_id).unwrap();
    assert!(
        !waiting,
        "Should not be waiting for channel name at Welcome step"
    );

    // Transition to CreateChannel (simulating user said "yes")
    let state2 = OnboardingState {
        step: OnboardingStep::CreateChannel,
        started_at: state1.started_at.clone(),
    };
    gorp::onboarding::save_state(&store, user_id, &state2).unwrap();

    // Now should be waiting for channel name
    let waiting = gorp::onboarding::is_waiting_for_channel_name(&store, user_id).unwrap();
    assert!(
        waiting,
        "Should be waiting for channel name at CreateChannel step"
    );
}

#[test]
fn scenario_state_persists_across_store_instances() {
    let temp_dir = tempfile::tempdir().unwrap();
    let template_dir = temp_dir.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let user_id = "@eve:example.com";

    // First store instance - save state
    {
        let store1 = SessionStore::new(temp_dir.path()).unwrap();
        let state = OnboardingState {
            step: OnboardingStep::CreateChannel,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        gorp::onboarding::save_state(&store1, user_id, &state).unwrap();
    }

    // Second store instance - state should persist
    {
        let store2 = SessionStore::new(temp_dir.path()).unwrap();
        let loaded = gorp::onboarding::get_state(&store2, user_id)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.step, OnboardingStep::CreateChannel);
        assert!(gorp::onboarding::is_waiting_for_channel_name(&store2, user_id).unwrap());
    }
}

// =============================================================================
// SCENARIO: Edge cases
// =============================================================================

#[test]
fn scenario_is_waiting_for_channel_name_false_when_no_state() {
    let (store, _temp) = create_test_store();
    let user_id = "@frank:example.com";

    // No state saved - should not be waiting
    let waiting = gorp::onboarding::is_waiting_for_channel_name(&store, user_id).unwrap();
    assert!(!waiting, "Should not be waiting when no state exists");
}

#[test]
fn scenario_is_waiting_for_channel_name_false_when_completed() {
    let (store, _temp) = create_test_store();
    let user_id = "@grace:example.com";

    let state = OnboardingState {
        step: OnboardingStep::Completed,
        started_at: "2024-01-01T00:00:00Z".to_string(),
    };
    gorp::onboarding::save_state(&store, user_id, &state).unwrap();

    let waiting = gorp::onboarding::is_waiting_for_channel_name(&store, user_id).unwrap();
    assert!(!waiting, "Should not be waiting when onboarding completed");
}

#[test]
fn scenario_get_state_returns_none_for_new_user() {
    let (store, _temp) = create_test_store();
    let user_id = "@henry:example.com";

    let state = gorp::onboarding::get_state(&store, user_id).unwrap();
    assert!(state.is_none(), "New user should have no state");
}

#[test]
fn scenario_all_steps_identified_correctly() {
    let (store, _temp) = create_test_store();
    let user_id = "@ivan:example.com";

    let steps = [
        (OnboardingStep::Welcome, true, false), // needs onboarding, not waiting
        (OnboardingStep::ApiKeyCheck, true, false), // needs onboarding, not waiting
        (OnboardingStep::CreateChannel, true, true), // needs onboarding, waiting
        (OnboardingStep::Completed, false, false), // no onboarding, not waiting
    ];

    for (step, expected_needs, expected_waiting) in steps {
        let state = OnboardingState {
            step: step.clone(),
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        gorp::onboarding::save_state(&store, user_id, &state).unwrap();

        let needs = gorp::onboarding::should_onboard(&store, user_id).unwrap();
        let waiting = gorp::onboarding::is_waiting_for_channel_name(&store, user_id).unwrap();

        assert_eq!(
            needs, expected_needs,
            "Step {:?}: should_onboard mismatch",
            step
        );
        assert_eq!(
            waiting, expected_waiting,
            "Step {:?}: is_waiting_for_channel_name mismatch",
            step
        );
    }
}
