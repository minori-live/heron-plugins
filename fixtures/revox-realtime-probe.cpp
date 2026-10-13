// Independent diagnostic of sustained packaged Revox processing through VST3.
// Keep capture free of analysis, allocation and logging; inspect every interval
// after all 30 seconds have been delivered against one absolute clock.
#include "pluginterfaces/base/ipluginbase.h"
#include "pluginterfaces/vst/ivstaudioprocessor.h"
#include "pluginterfaces/vst/ivstparameterchanges.h"
#include "revox-audio-probe.h"
#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <limits>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>
#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#else
#include <dlfcn.h>
#endif

namespace {
using namespace Steinberg;
using namespace Steinberg::Vst;
using Clock = std::chrono::steady_clock;
using revox_audio::block_size;
using revox_audio::sample_rate;
constexpr size_t sample_count = 30 * 48'000;
constexpr size_t window_samples = 24'000;
constexpr size_t block_count = (sample_count + block_size - 1) / block_size;
const double expected_hz = 220.0 * std::exp2(3.5 / 12.0);

void require(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

void check(tresult result, const char* operation) {
    if (result != kResultOk)
        throw std::runtime_error(std::string(operation) + ": " + std::to_string(result));
}

// The queues are borrowed for one synchronous process() call, as in revox-probe.
tresult query_borrowed(FUnknown* self, const TUID own, const TUID requested, void** object) {
    if (!object) return kInvalidArgument;
    *object = nullptr;
    if (std::memcmp(own, requested, sizeof(TUID)) != 0 &&
        std::memcmp(FUnknown_iid, requested, sizeof(TUID)) != 0)
        return kNoInterface;
    *object = self;
    return kResultOk;
}

class ParameterQueue final : public IParamValueQueue {
public:
    ParameterQueue(ParamID id, ParamValue value) : id_(id), value_(value) {}
    tresult PLUGIN_API queryInterface(const TUID iid, void** object) override {
        return query_borrowed(this, IParamValueQueue_iid, iid, object);
    }
    uint32 PLUGIN_API addRef() override { return 1; }
    uint32 PLUGIN_API release() override { return 1; }
    ParamID PLUGIN_API getParameterId() override { return id_; }
    int32 PLUGIN_API getPointCount() override { return 1; }
    tresult PLUGIN_API getPoint(int32 index, int32& offset, ParamValue& value) override {
        if (index != 0) return kInvalidArgument;
        offset = 0;
        value = value_;
        return kResultOk;
    }
    tresult PLUGIN_API addPoint(int32, ParamValue, int32&) override {
        return kNotImplemented;
    }
private:
    ParamID id_;
    ParamValue value_;
};

class PitchChanges final : public IParameterChanges {
public:
    // Stable parameter IDs: +3 semitones in [-12, 12], +50 cents in [-100, 100].
    PitchChanges() : queues_{ParameterQueue(0, 15.0 / 24.0), ParameterQueue(1, 150.0 / 200.0)} {}
    tresult PLUGIN_API queryInterface(const TUID iid, void** object) override {
        return query_borrowed(this, IParameterChanges_iid, iid, object);
    }
    uint32 PLUGIN_API addRef() override { return 1; }
    uint32 PLUGIN_API release() override { return 1; }
    int32 PLUGIN_API getParameterCount() override { return 2; }
    IParamValueQueue* PLUGIN_API getParameterData(int32 index) override {
        return index >= 0 && index < 2 ? &queues_[index] : nullptr;
    }
    IParamValueQueue* PLUGIN_API addParameterData(const ParamID&, int32& index) override {
        index = -1;
        return nullptr;
    }
private:
    std::array<ParameterQueue, 2> queues_;
};

struct LoadedPlugin {
#ifdef _WIN32
    HMODULE module = nullptr;
#else
    void* module = nullptr;
#endif
    IPluginFactory* factory = nullptr;
    IComponent* component = nullptr;
    IAudioProcessor* processor = nullptr;
    bool initialized = false;
    bool active = false;
    bool processing = false;

    ~LoadedPlugin() {
        if (processing) processor->setProcessing(false);
        if (active) component->setActive(false);
        if (initialized) component->terminate();
        if (processor) processor->release();
        if (component) component->release();
        if (factory) factory->release();
#ifdef _WIN32
        if (module) FreeLibrary(module);
#else
        if (module) dlclose(module);
#endif
    }

    void load(const char* path) {
#ifdef _WIN32
        module = LoadLibraryA(path);
        auto entry = module ? GetProcAddress(module, "GetPluginFactory") : nullptr;
#else
        module = dlopen(path, RTLD_NOW | RTLD_LOCAL);
        if (!module) throw std::runtime_error(dlerror());
        auto entry = dlsym(module, "GetPluginFactory");
#endif
        require(entry != nullptr, "could not load the packaged VST3 factory");
        factory = reinterpret_cast<IPluginFactory* (*)()>(entry)();
        require(factory != nullptr, "packaged VST3 returned no factory");
        for (int32 index = 0; index < factory->countClasses(); ++index) {
            PClassInfo info{};
            check(factory->getClassInfo(index, &info), "getClassInfo");
            if (std::strcmp(info.category, kVstAudioEffectClass) != 0) continue;
            check(factory->createInstance(info.cid, IComponent_iid,
                                          reinterpret_cast<void**>(&component)), "createInstance");
            break;
        }
        require(component != nullptr, "packaged VST3 has no audio component");
        check(component->initialize(nullptr), "initialize");
        initialized = true;
        check(component->queryInterface(IAudioProcessor_iid,
                                        reinterpret_cast<void**>(&processor)), "IAudioProcessor");
        require(processor != nullptr, "component returned no audio processor");
    }
};

struct Capture {
    std::array<std::vector<float>, 2> output{
        std::vector<float>(sample_count), std::vector<float>(sample_count)};
    std::vector<double> callback_ms = std::vector<double>(block_count);
    std::vector<double> feed_lateness_ms = std::vector<double>(block_count);
    uint32 latency = 0;
    size_t callback_overruns = 0;
    double wall_seconds = 0.0;
};

Clock::duration audio_duration(size_t frames) {
    return std::chrono::duration_cast<Clock::duration>(
        std::chrono::duration<double>(double(frames) / sample_rate));
}

void capture(const char* path, Capture& result) {
    // Precompute the exact release-probe harmonic signal. Separate mutable
    // channel buffers below prevent an in-place processor from aliasing them.
    std::vector<float> signal(sample_count);
    for (size_t index = 0; index < sample_count; ++index)
        signal[index] = revox_audio::input_sample(index);
    std::array<std::array<float, block_size>, 2> input{};
    std::array<std::array<float, block_size>, 2> output{};
    std::array<float*, 2> input_channels{input[0].data(), input[1].data()};
    std::array<float*, 2> output_channels{output[0].data(), output[1].data()};
    PitchChanges pitch;

    LoadedPlugin plugin;
    plugin.load(path);
    auto stereo = SpeakerArr::kStereo;
    check(plugin.processor->setBusArrangements(&stereo, 1, &stereo, 1), "setBusArrangements");
    check(plugin.component->activateBus(kAudio, kInput, 0, true), "activate input");
    check(plugin.component->activateBus(kAudio, kOutput, 0, true), "activate output");
    ProcessSetup setup{kRealtime, kSample32, block_size, sample_rate};
    check(plugin.processor->setupProcessing(setup), "setupProcessing");
    check(plugin.component->setActive(true), "setActive");
    plugin.active = true;
    check(plugin.processor->setProcessing(true), "setProcessing");
    plugin.processing = true;
    result.latency = plugin.processor->getLatencySamples();

    AudioBusBuffers input_bus{}, output_bus{};
    input_bus.numChannels = output_bus.numChannels = 2;
    input_bus.channelBuffers32 = input_channels.data();
    output_bus.channelBuffers32 = output_channels.data();
    ProcessData data{};
    data.processMode = kRealtime;
    data.symbolicSampleSize = kSample32;
    data.numInputs = data.numOutputs = 1;
    data.inputs = &input_bus;
    data.outputs = &output_bus;
    std::printf("Revox realtime capture: binary=%s, mode=kRealtime, channels=2, "
                "rate=48000, block=257, duration=30s, shift=+3.5, latency=%u\n",
                path, static_cast<unsigned>(result.latency));
    std::fflush(stdout);

    const auto started = Clock::now();
    for (size_t start = 0, block = 0; start < sample_count; start += block_size, ++block) {
        data.numSamples = static_cast<int32>(std::min(size_t(block_size), sample_count - start));
        data.inputParameterChanges = start == 0 ? &pitch : nullptr;
        input_bus.silenceFlags = output_bus.silenceFlags = 0;
        for (size_t channel = 0; channel < 2; ++channel)
            std::copy_n(signal.begin() + start, data.numSamples, input[channel].begin());
        // Preparation and the previous callback both count toward the next
        // deadline. A delayed wakeup never shifts the rest of the schedule.
        const auto deadline = started + audio_duration(start);
        std::this_thread::sleep_until(deadline);
        const auto callback_started = Clock::now();
        const auto status = plugin.processor->process(data);
        const auto callback_ended = Clock::now();
        result.callback_ms[block] = std::chrono::duration<double, std::milli>(
            callback_ended - callback_started).count();
        result.feed_lateness_ms[block] = std::max(0.0, std::chrono::duration<double, std::milli>(
            callback_started - deadline).count());
        if (result.callback_ms[block] > double(data.numSamples) * 1000.0 / sample_rate)
            ++result.callback_overruns;
        check(status, "process");
        for (size_t channel = 0; channel < 2; ++channel)
            std::copy_n(output[channel].begin(), data.numSamples, result.output[channel].begin() + start);
    }
    // Include the duration represented by the final partial audio block.
    std::this_thread::sleep_until(started + audio_duration(sample_count));
    result.wall_seconds = std::chrono::duration<double>(Clock::now() - started).count();
    // LoadedPlugin stops/joins the worker before pitch analysis can compete for
    // CPU or its stdout reports can interleave with capture diagnostics.
}

struct PitchMeasurement {
    double hz = std::numeric_limits<double>::quiet_NaN();
    double cents = std::numeric_limits<double>::quiet_NaN();
    double correlation = 0.0;
    double rms = 0.0;
    size_t non_finite = 0;

    bool shifted() const {
        return non_finite == 0 && rms > 0.0001 &&
               std::isfinite(cents) && std::abs(cents) < 25.0;
    }
};

PitchMeasurement measure_pitch(const std::vector<float>& output, size_t start) {
    PitchMeasurement result;
    double energy = 0.0;
    for (size_t index = start; index < start + window_samples; ++index) {
        if (!std::isfinite(output[index])) ++result.non_finite;
        energy += double(output[index]) * output[index];
    }
    result.rms = std::sqrt(energy / window_samples);
    if (result.non_finite != 0 || result.rms <= 0.0001) return result;
    double best = -1.0;
    int best_lag = 0;
    // Match the original release probe's integer-lag oracle and 25-cent bound.
    // Both delayed dry (220 Hz) and coarse-only (+3) remain rejected.
    for (int lag = 140; lag < 320; ++lag) {
        double xy = 0.0, xx = 0.0, yy = 0.0;
        for (size_t index = start; index < start + window_samples - 320; ++index) {
            const double a = output[index];
            const double b = output[index + lag];
            xy += a * b;
            xx += a * a;
            yy += b * b;
        }
        if (xx == 0.0 || yy == 0.0) continue;
        const double correlation = xy / std::sqrt(xx * yy);
        if (correlation > best) {
            best = correlation;
            best_lag = lag;
        }
    }
    if (best_lag != 0) {
        result.hz = sample_rate / best_lag;
        result.cents = 1200.0 * std::log2(result.hz / expected_hz);
        result.correlation = best;
    }
    return result;
}

double percentile99(std::vector<double>& values) {
    std::sort(values.begin(), values.end());
    return values[static_cast<size_t>(std::ceil(double(values.size()) * 0.99)) - 1];
}

bool analyze(Capture& captured) {
    const double callback_p99 = percentile99(captured.callback_ms);
    const double lateness_p99 = percentile99(captured.feed_lateness_ms);
    std::printf("Revox realtime timing: blocks=%zu, audio_seconds=30.000000, wall_seconds=%.6f, "
                "callback_max_ms=%.6f, callback_p99_ms=%.6f, callback_overruns=%zu, "
                "max_feed_lateness_ms=%.6f, feed_lateness_p99_ms=%.6f\n",
                block_count, captured.wall_seconds, captured.callback_ms.back(), callback_p99,
                captured.callback_overruns, captured.feed_lateness_ms.back(), lateness_p99);

    bool valid = captured.latency == 71'168;
    size_t pre_latency_nonzero = 0;
    for (const auto& channel : captured.output) {
        for (size_t index = 0; index < std::min(size_t(captured.latency), channel.size()); ++index)
            if (channel[index] != 0.0f) ++pre_latency_nonzero;
    }
    valid = valid && pre_latency_nonzero == 0;
    // The old probe starts inspecting at latency + 0.5 s (1.9827 s).
    // The first full aligned half-second interval after that point is 2.0 s.
    const size_t stable_start = ((size_t(captured.latency) + window_samples * 2 - 1) /
                                 window_samples) * window_samples;
    size_t failed_stable_windows = 0;
    size_t stable_windows = 0;
    size_t total_non_finite = 0;
    for (size_t start = 0; start < sample_count; start += window_samples) {
        const bool stable = start >= stable_start;
        bool both_shifted = true;
        for (size_t channel = 0; channel < 2; ++channel) {
            const auto pitch = measure_pitch(captured.output[channel], start);
            total_non_finite += pitch.non_finite;
            both_shifted = both_shifted && pitch.shifted();
            std::printf("Revox realtime interval: start=%.1fs, end=%.1fs, channel=%zu, "
                        "stable=%s, pitch=%.3fHz, expected=%.3fHz, cents=%.2f, rms=%.5f, "
                        "correlation=%.6f, non_finite=%zu, shifted=%s\n",
                        double(start) / sample_rate, double(start + window_samples) / sample_rate,
                        channel, stable ? "true" : "false", pitch.hz, expected_hz, pitch.cents,
                        pitch.rms, pitch.correlation, pitch.non_finite, pitch.shifted() ? "true" : "false");
        }
        if (stable) {
            ++stable_windows;
            if (!both_shifted) ++failed_stable_windows;
        }
    }
    valid = valid && total_non_finite == 0 && stable_windows != 0 && failed_stable_windows == 0;
    std::printf("Revox realtime result: %s, latency=%u, pre_latency_nonzero=%zu, non_finite=%zu, "
                "stable_from=%.1fs, stable_windows=%zu, failed_stable_windows=%zu\n",
                valid ? "PASS" : "FAIL", static_cast<unsigned>(captured.latency), pre_latency_nonzero,
                total_non_finite, double(stable_start) / sample_rate, stable_windows, failed_stable_windows);
    return valid;
}
} // namespace

int main(int argc, char** argv) {
    if (argc != 2) {
        std::fprintf(stderr, "usage: revox-realtime-probe <packaged VST3 binary>\n");
        return 2;
    }
    try {
        Capture captured;
        capture(argv[1], captured);
        return analyze(captured) ? 0 : 1;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "Revox realtime diagnostic failed: %s\n", error.what());
        return 1;
    }
}
