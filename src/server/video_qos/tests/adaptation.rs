use super::*;

const DISPLAY: &str = "adaptation";

fn session(abr: bool) -> VideoQoS {
    let mut qos = super::smoke::session(FPS, Quality::Balanced);
    qos.abr_config = abr;
    qos.new_display(DISPLAY.to_owned());
    qos.set_support_changing_quality(DISPLAY, true);
    sync_bitrate(&mut qos);
    qos
}

fn sync_bitrate(qos: &mut VideoQoS) {
    let bitrate = (6000.0 * qos.ratio()) as u32;
    qos.store_bitrate(bitrate);
}

fn second(qos: &mut VideoQoS, delay: u32, dynamic: bool) {
    let encoded = if dynamic { qos.fps() as usize } else { 0 };
    qos.advance_ms(1000);
    sync_bitrate(qos);
    qos.user_network_delay(1, delay);
    sync_bitrate(qos);
    qos.update_display_data(DISPLAY, encoded);
    sync_bitrate(qos);
}

#[test]
fn static_to_dynamic_ratio_recovery() {
    println!("Static recovery: 90 s healthy video, 12 s confirmed 800 ms delay, 60 s healthy static screen, then 90 s video. Bitrate is modeled as ratio * 6000 kbps.");
    println!("| restart profile | ratio after cut | ratio after static | time to 90% | time to 95% | final ratio | final FPS | modeled bitrate |");
    println!("|---|---:|---:|---|---|---:|---:|---:|");
    for (profile, restart_delay, growing_queue) in [
        ("healthy 10 ms", 10, false),
        ("stable path 800 ms", 800, false),
        ("growing queue 800 + 10 ms/s", 800, true),
    ] {
        let mut qos = session(true);
        for _ in 0..90 {
            second(&mut qos, 10, true);
        }
        let target = qos.latest_quality().ratio();
        assert_eq!(qos.ratio(), target);
        for _ in 0..12 {
            second(&mut qos, 800, true);
        }
        let after_cut = qos.ratio();
        assert!(
            after_cut < target * 0.5,
            "fixture must confirm congestion and cut bitrate"
        );
        for _ in 0..60 {
            second(&mut qos, 10, false);
        }
        let after_static = qos.ratio();
        let mut t90 = (after_static >= target * 0.90).then_some(0);
        let mut t95 = (after_static >= target * 0.95).then_some(0);
        for s in 1..=90 {
            let delay = restart_delay + if growing_queue { s * 10 } else { 0 };
            second(&mut qos, delay, true);
            let ratio = qos.ratio();
            if ratio >= target * 0.90 {
                t90.get_or_insert(s);
            }
            if ratio >= target * 0.95 {
                t95.get_or_insert(s);
            }
            if growing_queue {
                assert!(
                    ratio <= after_static * 1.02,
                    "activity must not restore quality into congestion"
                );
            }
        }
        let seconds = |time: Option<u32>| {
            time.map(|s| format!("{s} s"))
                .unwrap_or_else(|| "never".to_owned())
        };
        let ratio = qos.ratio();
        println!("| {profile} | {after_cut:.3} | {after_static:.3} | {} | {} | {ratio:.3} | {} | {} kbps |",
            seconds(t90), seconds(t95), qos.fps(), qos.bitrate());
        if !growing_queue {
            assert!(
                t95.is_some(),
                "{profile}: video did not regain 95% quality within 90 s"
            );
            assert_eq!(qos.fps(), FPS);
        }
    }
}

#[test]
fn shedding_frames_lowers_quality_while_the_probe_stays_healthy() {
    // Shedding is congestion the round trip cannot report: the queue stays short because the
    // frames were thrown away, so every delay sample reads like a healthy link. Without the
    // shed evidence the controller would hold both bitrate and rate and go on shedding.
    let mut qos = session(true);
    for _ in 0..90 {
        second(&mut qos, 10, true);
    }
    let target = qos.latest_quality().ratio();
    assert_eq!(qos.ratio(), target);

    // Every interval sheds, and every delay reply still arrives at 10 ms.
    for _ in 0..30 {
        for _ in 0..2 {
            qos.user_video_frame_shed(1);
        }
        second(&mut qos, 10, true);
    }
    assert!(
        qos.ratio() < target * 0.9,
        "a link that keeps shedding must lose bitrate although its delay reads healthy: {} -> {}",
        target,
        qos.ratio()
    );

    // And the shedding stops once there is nothing left to prove: no further cut.
    let after_shedding = qos.ratio();
    for _ in 0..30 {
        second(&mut qos, 10, true);
    }
    assert!(
        qos.ratio() >= after_shedding,
        "quality must recover once frames are no longer shed"
    );
}

#[test]
fn shedding_frames_lowers_the_rate_when_the_bitrate_cannot() {
    // With no bitrate to give back, the frame rate is the only lever left. Shedding still has
    // to be answered, or the link keeps dropping frames at a rate it was told to deliver.
    let mut qos = session(false);
    for _ in 0..90 {
        second(&mut qos, 10, true);
    }
    assert_eq!(qos.fps(), FPS);
    for _ in 0..2 {
        qos.user_video_frame_shed(1);
    }
    second(&mut qos, 10, true);
    assert!(
        qos.fps() < FPS,
        "shedding must lower the rate when the bitrate cannot absorb it: {}",
        qos.fps()
    );
}
