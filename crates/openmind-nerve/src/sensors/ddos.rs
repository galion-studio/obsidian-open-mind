// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Request-rate streams: publishes each [`crate::access::DdosVerdict`] in the
//! cortex `ddos.streams` section; `flood` raises the DDoS flag and a critical
//! `ddos:<stream>` signal, `elevated` a warn signal. Reads [`Input::ddos`].
//! (The detector itself stays in [`crate::access`]. Per-hostname `edge-host:*`
//! streams (T2.12) are listed but signalled by [`super::edgerate::EdgeRate`].)

use super::{Sensor, Sink};
use crate::cortex::Input;
use crate::incident::{Sev, Signal};

/// Built-in sensor `ddos`.
pub struct RateStreams;

impl Sensor for RateStreams {
    fn name(&self) -> &'static str {
        "ddos"
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        for v in i.ddos {
            #[cfg(feature = "sensor-edgerate")]
            if v.stream.starts_with(super::edgerate::PREFIX) {
                // Per-hostname edge streams (T2.12): listed here, signalled once
                // by the `edge-rate` sensor under the tunnel cause.
                out.ddos_stream(v.to_json(), v.level == "flood");
                continue;
            }
            let target = v.component.clone().unwrap_or_else(|| v.stream.clone());
            let (sev, what) = match v.level {
                "flood" => (Sev::Critical, "request flood"),
                "elevated" => (Sev::Warn, "request rate elevated"),
                _ => {
                    out.ddos_stream(v.to_json(), false);
                    continue;
                }
            };
            out.ddos_stream(v.to_json(), v.level == "flood");
            out.signal(Signal::new(
                &format!("ddos:{}", v.stream),
                &target,
                sev,
                format!("{what}: {}", v.explain()),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::DdosVerdict;
    use crate::sensors::Sensors;

    fn verdict(stream: &str, level: &'static str) -> DdosVerdict {
        DdosVerdict {
            stream: stream.into(),
            component: Some("tuner".into()),
            level,
            rps: 120.0,
            requests_60s: 7200,
            baseline_rps: 1.0,
            mad_rps: 0.5,
            baseline_buckets: 30,
            threshold_rps: 10.0,
            factor: 120.0,
            shape: "single-source",
            top_ips: vec![],
            distinct_ips: 1,
            error_share: 0.0,
            scan_hits_60s: 0,
            flood_since: None,
            floods_total: 1,
            peak_rps: 120.0,
            total: 7200,
        }
    }

    #[test]
    fn flood_fires_elevated_warns_normal_is_silent() {
        let vs = [
            verdict("a", "flood"),
            verdict("b", "elevated"),
            verdict("c", "normal"),
        ];
        let logs = crate::logscan::LogInsights::default();
        let mut i = Input::new(1, &[], &logs);
        i.ddos = &vs;
        let mut s = Sensors::empty();
        s.add(RateStreams);
        let cx = s.run(&i);
        assert!(cx.ddos_firing);
        assert_eq!(cx.ddos.len(), 3);
        let sigs: Vec<(&str, Sev)> = cx
            .signals
            .iter()
            .map(|s| (s.cause.as_str(), s.sev))
            .collect();
        assert_eq!(sigs, [("ddos:a", Sev::Critical), ("ddos:b", Sev::Warn)]);
        assert_eq!(cx.signals[0].component, "tuner");
        i.ddos = &vs[1..];
        assert!(!s.run(&i).ddos_firing);
    }
}
