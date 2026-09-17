//! The renderer: semantic state + resolved mapping -> DMX buffers.
//!
//! This is a pure, deterministic transform. It:
//!
//! 1. reads `state` (never mutates it);
//! 2. walks `rig.fixtures` in order, an already-resolved physical
//!    mapping it never second-guesses (no parsing, no name resolution,
//!    no role/rig-textual knowledge — see this module's parent docs);
//! 3. converts each mapped attribute's value to DMX8 (see
//!    [`crate::convert`]);
//! 4. writes it into the right [`UniverseFrame`] slot in `output`.
//!
//! It never talks to a USB device, a driver, or anything protocol-level
//! — that's `inception-driver-dmx`'s job, one layer further down.

use std::collections::HashMap;

use inception_core::{LightingState, UniverseId};

use crate::convert::u16_to_dmx8;
use crate::frame::UniverseFrame;
use crate::mapping::ResolvedRig;

/// Renders `state` through `rig` into `output`, creating a fresh
/// [`UniverseFrame`] (starting from [`UniverseFrame::black`]) for any
/// universe touched that `output` doesn't already have an entry for.
///
/// Determinism (item 38 of the task brief): `output` is a `HashMap`, but
/// its iteration order never affects the result — every write here is a
/// keyed `entry(universe)` lookup, driven by `rig.fixtures`'s fixed
/// `Vec` order, not by iterating the map itself.
pub fn render(
    state: &LightingState,
    rig: &ResolvedRig,
    output: &mut HashMap<UniverseId, UniverseFrame>,
) {
    for fixture in &rig.fixtures {
        if let Some(mapping) = fixture.intensity {
            let value = u16_to_dmx8(state.intensity(fixture.id).raw());
            output
                .entry(mapping.universe)
                .or_insert_with(UniverseFrame::black)
                .set(mapping.channel, value);
        }

        if let Some(mapping) = fixture.color {
            let color = state.color(fixture.id);
            let frame = output
                .entry(mapping.universe)
                .or_insert_with(UniverseFrame::black);
            frame.set(mapping.red, u16_to_dmx8(color.red));
            frame.set(mapping.green, u16_to_dmx8(color.green));
            frame.set(mapping.blue, u16_to_dmx8(color.blue));
        }

        if let Some(mapping) = fixture.strobe {
            let value = u16_to_dmx8(state.strobe(fixture.id).raw());
            output
                .entry(mapping.universe)
                .or_insert_with(UniverseFrame::black)
                .set(mapping.channel, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use inception_core::{AttributeValue, FixtureId, Intensity, ResolvedTarget, TargetId};

    use super::*;
    use crate::dmx_channel::DmxChannel;
    use crate::mapping::{DmxChannelMapping, ResolvedFixture};

    fn channel(n: u16) -> DmxChannel {
        DmxChannel::new(n).unwrap()
    }

    #[test]
    fn two_fixtures_in_one_universe_land_on_their_own_channels() {
        // The exact scenario from item 32 of the task brief.
        let rig = ResolvedRig {
            fixtures: vec![
                ResolvedFixture {
                    id: FixtureId(0),
                    intensity: Some(DmxChannelMapping {
                        universe: UniverseId(1),
                        channel: channel(1),
                    }),
                    color: None,
                    strobe: None,
                },
                ResolvedFixture {
                    id: FixtureId(1),
                    intensity: Some(DmxChannelMapping {
                        universe: UniverseId(1),
                        channel: channel(5),
                    }),
                    color: None,
                    strobe: None,
                },
            ],
        };

        let mut state = LightingState::new();
        state.define_target(
            TargetId(0),
            ResolvedTarget {
                fixtures: vec![FixtureId(0), FixtureId(1)],
            },
        );
        state
            .set_target_attribute(
                TargetId(0),
                AttributeValue::Intensity(Intensity::from_percent(50).unwrap()),
            )
            .unwrap();

        let mut output = HashMap::new();
        render(&state, &rig, &mut output);

        let frame = &output[&UniverseId(1)];
        let expected = crate::convert::intensity_to_dmx8(Intensity::from_percent(50).unwrap());
        assert_eq!(frame[0], expected);
        assert_eq!(frame[4], expected);

        // Every other slot stays at 0.
        for (index, &slot) in frame.as_slice().iter().enumerate() {
            if index != 0 && index != 4 {
                assert_eq!(slot, 0, "channel {} should be untouched", index + 1);
            }
        }
    }

    #[test]
    fn fixtures_in_different_universes_produce_independent_frames() {
        let rig = ResolvedRig {
            fixtures: vec![
                ResolvedFixture {
                    id: FixtureId(0),
                    intensity: Some(DmxChannelMapping {
                        universe: UniverseId(1),
                        channel: channel(1),
                    }),
                    color: None,
                    strobe: None,
                },
                ResolvedFixture {
                    id: FixtureId(1),
                    intensity: Some(DmxChannelMapping {
                        universe: UniverseId(2),
                        channel: channel(1),
                    }),
                    color: None,
                    strobe: None,
                },
            ],
        };

        let mut state = LightingState::new();
        state.set_fixture_attribute(FixtureId(0), AttributeValue::Intensity(Intensity::MAX));
        state.set_fixture_attribute(FixtureId(1), AttributeValue::Intensity(Intensity::ZERO));

        let mut output = HashMap::new();
        render(&state, &rig, &mut output);

        assert_eq!(output.len(), 2);
        assert_eq!(output[&UniverseId(1)][0], 255);
        assert_eq!(output[&UniverseId(2)][0], 0);
    }

    #[test]
    fn color_attribute_fills_three_channels() {
        let rig = ResolvedRig {
            fixtures: vec![ResolvedFixture {
                id: FixtureId(0),
                intensity: None,
                color: Some(crate::mapping::RgbChannelMapping {
                    universe: UniverseId(1),
                    red: channel(1),
                    green: channel(2),
                    blue: channel(3),
                }),
                strobe: None,
            }],
        };

        let mut state = LightingState::new();
        state.set_fixture_attribute(
            FixtureId(0),
            AttributeValue::Color(inception_core::Rgb {
                red: 65535,
                green: 0,
                blue: 32768,
            }),
        );

        let mut output = HashMap::new();
        render(&state, &rig, &mut output);

        let frame = &output[&UniverseId(1)];
        assert_eq!(frame[0], 255);
        assert_eq!(frame[1], 0);
        assert_eq!(frame[2], u16_to_dmx8(32768));
    }

    #[test]
    fn strobe_attribute_lands_on_its_own_channel_independent_of_intensity() {
        let rig = ResolvedRig {
            fixtures: vec![ResolvedFixture {
                id: FixtureId(0),
                intensity: Some(DmxChannelMapping {
                    universe: UniverseId(1),
                    channel: channel(1),
                }),
                color: None,
                strobe: Some(DmxChannelMapping {
                    universe: UniverseId(1),
                    channel: channel(2),
                }),
            }],
        };

        let mut state = LightingState::new();
        state.set_fixture_attribute(FixtureId(0), AttributeValue::Intensity(Intensity::MAX));
        state.set_fixture_attribute(
            FixtureId(0),
            AttributeValue::Strobe(Intensity::from_percent(50).unwrap()),
        );

        let mut output = HashMap::new();
        render(&state, &rig, &mut output);

        let frame = &output[&UniverseId(1)];
        assert_eq!(frame[0], 255);
        assert_eq!(
            frame[1],
            crate::convert::intensity_to_dmx8(Intensity::from_percent(50).unwrap())
        );
    }

    #[test]
    fn rendering_is_deterministic_across_runs() {
        let rig = ResolvedRig {
            fixtures: vec![ResolvedFixture {
                id: FixtureId(0),
                intensity: Some(DmxChannelMapping {
                    universe: UniverseId(1),
                    channel: channel(1),
                }),
                color: None,
                strobe: None,
            }],
        };
        let mut state = LightingState::new();
        state.set_fixture_attribute(
            FixtureId(0),
            AttributeValue::Intensity(Intensity::from_percent(37).unwrap()),
        );

        let mut first = HashMap::new();
        render(&state, &rig, &mut first);
        let mut second = HashMap::new();
        render(&state, &rig, &mut second);

        assert_eq!(first, second);
    }
}
