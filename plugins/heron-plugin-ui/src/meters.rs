//! Neutral stereo peak meters shared with the EQ status bar.

use std::{fmt::Debug, marker::PhantomData};

use truce::{core::util::meter_display, params::Params};
use truce_iced::{
    Message, ParamCache,
    iced::{
        Border, Element, Length,
        widget::{Row, progress_bar},
    },
};

use crate::palette;

/// Create a peak meter from the supplied parameter-cache meter IDs.
pub fn level_meter<'a, M: Clone + Debug + 'static>(
    ids: &[impl Into<u32> + Copy],
    params: &'a ParamCache<impl Params>,
) -> LevelMeter<'a, M> {
    LevelMeter::new(ids.iter().map(|id| params.meter((*id).into())).collect())
}

/// Create a meter from linear peak amplitudes reported by an editor's telemetry.
pub fn level_meter_values<'a, M: Clone + Debug + 'static>(values: &[f32]) -> LevelMeter<'a, M> {
    LevelMeter::new(values.to_vec())
}

pub struct LevelMeter<'a, M> {
    values: Vec<f32>,
    width: Length,
    height: Length,
    marker: PhantomData<(&'a (), M)>,
}

impl<'a, M: Clone + Debug + 'static> LevelMeter<'a, M> {
    fn new(values: Vec<f32>) -> Self {
        Self {
            values,
            width: Length::Fixed(16.0),
            height: Length::Fixed(80.0),
            marker: PhantomData,
        }
    }

    #[must_use]
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = Length::Fixed(width);
        self.height = Length::Fixed(height);
        self
    }

    #[must_use]
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    #[must_use]
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    #[must_use]
    pub fn fill(mut self) -> Self {
        self.height = Length::Fill;
        self
    }

    #[must_use]
    pub fn into_element(self) -> Element<'a, Message<M>> {
        let colors = palette();
        let mut bars = Row::new().spacing(3).width(self.width).height(self.height);
        for peak in self.values {
            bars = bars.push(
                progress_bar(0.0..=1.0, meter_display(peak))
                    .vertical()
                    .length(self.height)
                    .girth(Length::Fill)
                    .style(move |_| progress_bar::Style {
                        background: colors.control.into(),
                        bar: if peak > 1.0 {
                            colors.danger
                        } else {
                            colors.audio
                        }
                        .into(),
                        border: Border {
                            radius: 2.0.into(),
                            ..Default::default()
                        },
                    }),
            );
        }
        bars.into()
    }
}

impl<'a, M: Clone + Debug + 'static> From<LevelMeter<'a, M>> for Element<'a, Message<M>> {
    fn from(meter: LevelMeter<'a, M>) -> Self {
        meter.into_element()
    }
}
