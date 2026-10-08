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
fn remapping_requests_the_next_gtk_frame_without_extra_delay() {
    let mut schedule = PaintSchedule::default();
    schedule.painted(10);
    schedule.damage(11, &[7]);
    assert_eq!(schedule.next_delay_ms_if_mapped(12, false), None);
    assert_eq!(schedule.next_delay_ms_if_mapped(12, true), Some(0));
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
    assert!(schedule.queue_due_paint(10_001, true));
    assert!(!schedule.queue_due_paint(10_016, true));
}

#[test]
fn echoed_input_can_reach_the_next_gtk_frame_after_recent_output() {
    // GTK already owns the frame clock. A frame starting at t=0 finishes
    // painting at t=4; the next echoed input arrives at t=5. An additional
    // 16 ms wait would miss even a 60 Hz frame, and worse at 120/144 Hz.
    for next_frame_ms in [17, 8, 7] {
        let mut schedule = PaintSchedule::default();
        schedule.painted(4);
        schedule.damage(5, &[3]);
        let request_at = 5 + schedule.next_delay_ms_if_mapped(5, true).unwrap();
        assert!(
            request_at < next_frame_ms,
            "echo queued at {request_at} ms misses GTK frame at {next_frame_ms} ms"
        );
        assert!(schedule.queue_due_paint(request_at, true));
        schedule.damage(6, &[4]);
        assert!(
            !schedule.queue_due_paint(6, true),
            "GTK already has a frame queued"
        );
        assert_eq!(schedule.pending_lines(), vec![3, 4]);
    }
}
