// Shared owned-audio assertions for the final VST3 and AU Revox bundles.
#pragma once
#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <stdexcept>
#include <vector>

namespace revox_audio {
constexpr double sample_rate = 48'000.0;
constexpr int block_size = 257;
constexpr size_t sample_count = 192'000;

inline void require(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

inline float input_sample(size_t position) noexcept {
    constexpr double pi = 3.14159265358979323846;
    const double phase = 2.0 * pi * 220.0 * double(position) / sample_rate;
    return static_cast<float>(0.15 * std::sin(phase) + 0.07 * std::sin(2.0 * phase)
                              + 0.04 * std::sin(3.0 * phase));
}

inline void verify_audio(const std::vector<float>& output, uint32_t latency, const char* format) {
    require(latency == 71'168, "packaged latency differs from 1480 ms + 128 samples");
    require(output.size() >= size_t(latency) + 48'000, "packaged audio is too short to inspect");
    require(std::all_of(output.begin(), output.end(), [](float value) { return std::isfinite(value); }),
            "packaged Revox produced non-finite audio");
    require(std::all_of(output.begin(), output.begin() + latency, [](float value) { return value == 0.0f; }),
            "audio appeared before the reported latency");

    // Inspect a steady interval beyond startup and the delayed dry/wet transition.
    const size_t start = latency + 24'000;
    const size_t length = 24'000;
    double energy = 0.0;
    for (size_t index = start; index < start + length; ++index)
        energy += double(output[index]) * output[index];
    const double rms = std::sqrt(energy / length);
    require(rms > 0.0001, "packaged Revox produced no measurable voice signal");

    double best = -1.0;
    int best_lag = 0;
    for (int lag = 140; lag < 320; ++lag) {
        double xy = 0.0, xx = 0.0, yy = 0.0;
        for (size_t index = start; index < start + length - 320; ++index) {
            const double a = output[index];
            const double b = output[index + lag];
            xy += a * b;
            xx += a * a;
            yy += b * b;
        }
        const double correlation = xy / std::sqrt(xx * yy);
        if (correlation > best) {
            best = correlation;
            best_lag = lag;
        }
    }
    require(best_lag != 0, "could not measure packaged Revox pitch");
    const double measured = sample_rate / best_lag;
    const double expected = 220.0 * std::exp2(3.5 / 12.0);
    const double cents = 1200.0 * std::log2(measured / expected);
    std::printf("Packaged Revox %s: latency=%u samples, pitch=%.3f Hz, expected=%.3f Hz, error=%.2f cents, rms=%.5f\n",
                format, static_cast<unsigned>(latency), measured, expected, cents, rms);
    // An initialization failure falls back to 220 Hz delayed dry audio. Ignoring
    // the fine control gives +3 semitones, 50 cents below the requested pitch.
    // A 25-cent bound rejects both cases despite integer-lag quantization.
    require(std::abs(cents) < 25.0, "packaged Revox did not apply the combined +3.5 semitone shift");
}
} // namespace revox_audio
