//! Sound out of the computer's speakers, for the built-in Spectrum: samples are queued as the emulator makes
//! them and played as the system asks for them. If there's no sound device, there's simply no sound.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// At most this much sound waits to be played, so it never falls behind the picture.
const MOST_WAITING_SECONDS: f32 = 0.15;

pub struct Speaker {
    _stream: cpal::Stream,
    /// Left and right samples, one after the other.
    queue: Arc<Mutex<VecDeque<f32>>>,
    pub rate: u32,
}

impl Speaker {
    /// The system's speakers, if it has any.
    pub fn open() -> Option<Speaker> {
        let device = cpal::default_host().default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let config = supported.config();
        let channels = usize::from(config.channels).max(1);
        let queue = Arc::new(Mutex::new(VecDeque::with_capacity(config.sample_rate as usize)));
        let stream = match supported.sample_format() {
            SampleFormat::F32 => play::<f32>(&device, &config, queue.clone(), channels),
            SampleFormat::I16 => play::<i16>(&device, &config, queue.clone(), channels),
            SampleFormat::U16 => play::<u16>(&device, &config, queue.clone(), channels),
            SampleFormat::I32 => play::<i32>(&device, &config, queue.clone(), channels),
            _ => None,
        }?;
        stream.play().ok()?;
        Some(Speaker { _stream: stream, queue, rate: config.sample_rate })
    }

    /// Queues one stereo sample; the oldest go if too much is waiting.
    pub fn push(&self, left: f32, right: f32) {
        let mut queue = self.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let most = (self.rate as f32 * MOST_WAITING_SECONDS) as usize * 2;
        while queue.len() >= most {
            queue.pop_front();
        }
        queue.push_back(left);
        queue.push_back(right);
    }

    /// Lets go of whatever's waiting, as when the machine is paused.
    pub fn clear(&self) {
        self.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
    }
}

fn play<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: &cpal::StreamConfig, queue: Arc<Mutex<VecDeque<f32>>>, channels: usize) -> Option<cpal::Stream> {
    device
        .build_output_stream::<T, _, _>(
            *config,
            move |data: &mut [T], _| {
                let mut queue = queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                for frame in data.chunks_mut(channels) {
                    // Silence when nothing's waiting.
                    let (left, right) = if queue.len() >= 2 { (queue.pop_front().unwrap_or(0.0), queue.pop_front().unwrap_or(0.0)) } else { (0.0, 0.0) };
                    for (channel, sample) in frame.iter_mut().enumerate() {
                        *sample = T::from_sample(if channel % 2 == 0 { left } else { right });
                    }
                }
            },
            |_| {},
            None,
        )
        .ok()
}
