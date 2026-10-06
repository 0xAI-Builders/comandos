use comandos_app::term::schedule::PaintSchedule;

#[test]
fn hidden_terminal_damage_does_not_arm_a_paint_timer() {
    let mut schedules: Vec<_> = (0..20).map(|_| PaintSchedule::default()).collect();
    for schedule in &mut schedules {
        schedule.damage(0, &[0, 1, 2]);
    }
    // Hidden notebook pages never receive draw(), which alone acknowledges
    // damage. Repeated IO/exit checks must retain damage without 1 ms wakeups.
    for now in (0..10_000).step_by(100) {
        for schedule in &schedules {
            assert_eq!(schedule.next_delay_ms_if_mapped(now, false), None);
            assert_eq!(schedule.pending_lines(), vec![0, 1, 2]);
        }
    }
    for schedule in &mut schedules {
        assert_eq!(schedule.next_delay_ms_if_mapped(10_000, true), Some(0));
        schedule.painted(10_000);
        assert_eq!(schedule.pending_lines(), Vec::<usize>::new());
        assert_eq!(schedule.next_delay_ms_if_mapped(10_000, true), None);
    }
}

#[test]
fn remapping_retains_the_visible_frame_deadline() {
    let mut schedule = PaintSchedule::default();
    schedule.painted(10);
    schedule.damage(11, &[7]);
    assert_eq!(schedule.next_delay_ms_if_mapped(12, false), None);
    assert_eq!(schedule.next_delay_ms_if_mapped(12, true), Some(14));
    assert_eq!(schedule.next_delay_ms_if_mapped(26, true), Some(0));
    assert_eq!(schedule.pending_lines(), vec![7]);
}

#[test]
fn queued_paint_waits_for_gtk_without_losing_new_damage() {
    let mut schedule = PaintSchedule::default();
    schedule.damage(0, &[1]);
    assert!(schedule.queue_due_paint(0, true));
    schedule.damage(1, &[2]);
    for now in 1..10_000 {
        assert_eq!(schedule.next_delay_ms_if_mapped(now, true), None);
        assert!(!schedule.queue_due_paint(now, true));
    }
    assert_eq!(schedule.pending_lines(), vec![1, 2]);
    schedule.reset_queued_paint();
    assert_eq!(schedule.next_delay_ms_if_mapped(10_000, false), None);
    assert!(schedule.queue_due_paint(10_000, true));
    schedule.painted(10_000);
    assert_eq!(schedule.next_delay_ms_if_mapped(10_000, true), None);
    schedule.damage(10_001, &[3]);
    assert!(!schedule.queue_due_paint(10_001, true));
    assert!(schedule.queue_due_paint(10_016, true));
}
