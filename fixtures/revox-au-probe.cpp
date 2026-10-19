// Proves the AU wrapper serializes a host reset with an active Revox render.
#include <AudioToolbox/AudioToolbox.h>
#include "revox-audio-probe.h"
#include <array>
#include <chrono>
#include <condition_variable>
#include <cstdlib>
#include <future>
#include <limits>
#include <mutex>
#include <string>
#include <thread>

namespace {
using namespace std::chrono_literals;
using revox_audio::block_size;
using revox_audio::require;
using revox_audio::sample_count;
using revox_audio::sample_rate;

void check(OSStatus status, const char* operation) {
    if (status != noErr)
        throw std::runtime_error(std::string(operation) + ": " + std::to_string(status));
}

constexpr UInt32 fourcc(const char (&text)[5]) {
    return (UInt32(text[0]) << 24) | (UInt32(text[1]) << 16)
           | (UInt32(text[2]) << 8) | UInt32(text[3]);
}

// A broken native render cannot be unwound or joined by the host. Bound this
// probe process so an actual deadlock fails validation instead of hanging it.
class CompletionDeadline {
    std::mutex mutex;
    std::condition_variable condition;
    bool completed = false;
    std::thread watchdog;

public:
    CompletionDeadline() : watchdog([this] {
        std::unique_lock<std::mutex> lock(mutex);
        if (!condition.wait_for(lock, 60s, [this] { return completed; })) {
            std::fputs("Revox AU reset probe exceeded its native completion deadline\n", stderr);
            std::_Exit(EXIT_FAILURE);
        }
    }) {}

    ~CompletionDeadline() {
        {
            std::lock_guard<std::mutex> lock(mutex);
            completed = true;
        }
        condition.notify_all();
        watchdog.join();
    }
};

struct InputSource {
    std::array<float, block_size> samples{};
    size_t gate_position = 0;
    std::mutex mutex;
    std::condition_variable condition;
    bool entered = false;
    bool released = false;

    void release() {
        {
            std::lock_guard<std::mutex> lock(mutex);
            released = true;
        }
        condition.notify_all();
    }

    static OSStatus render(void* context, AudioUnitRenderActionFlags* flags,
                           const AudioTimeStamp* time, UInt32 bus, UInt32 frames,
                           AudioBufferList* data) noexcept {
        if (!context || !time || !data || bus != 0 || frames > block_size
            || data->mNumberBuffers != 1
            || !(time->mFlags & kAudioTimeStampSampleTimeValid)
            || !std::isfinite(time->mSampleTime) || time->mSampleTime < 0.0)
            return kAudioUnitErr_InvalidParameter;
        auto& source = *static_cast<InputSource*>(context);
        const auto start = static_cast<size_t>(time->mSampleTime);
        if (start == source.gate_position) {
            // The gate belongs to the test host, before Revox receives this
            // input. It keeps AudioUnitRender active independently of model
            // warmup speed or inference scheduling.
            std::unique_lock<std::mutex> lock(source.mutex);
            source.entered = true;
            source.condition.notify_all();
            source.condition.wait(lock, [&source] { return source.released; });
        }
        for (UInt32 index = 0; index < frames; ++index)
            source.samples[index] = revox_audio::input_sample(start + index);
        data->mBuffers[0] = {1, frames * UInt32(sizeof(float)), source.samples.data()};
        if (flags) *flags &= ~kAudioUnitRenderAction_OutputIsSilence;
        return noErr;
    }
};

struct LoadedUnit {
    AudioComponentInstance instance = nullptr;
    bool initialized = false;

    ~LoadedUnit() {
        if (initialized) AudioUnitUninitialize(instance);
        if (instance) AudioComponentInstanceDispose(instance);
    }
};

OSStatus render_block(AudioComponentInstance unit, size_t start, UInt32 frames,
                      float* output) {
    std::fill_n(output, frames, std::numeric_limits<float>::quiet_NaN());
    AudioBufferList data{};
    data.mNumberBuffers = 1;
    data.mBuffers[0] = {1, frames * UInt32(sizeof(float)), output};
    AudioTimeStamp time{};
    time.mSampleTime = double(start);
    time.mFlags = kAudioTimeStampSampleTimeValid;
    AudioUnitRenderActionFlags flags = 0;
    const auto status = AudioUnitRender(unit, &flags, &time, 0, frames, &data);
    if (status != noErr) return status;
    if (data.mNumberBuffers != 1 || data.mBuffers[0].mNumberChannels != 1
        || !data.mBuffers[0].mData || data.mBuffers[0].mDataByteSize < frames * sizeof(float))
        return kAudioUnitErr_InvalidParameter;
    std::copy_n(static_cast<const float*>(data.mBuffers[0].mData), frames, output);
    return noErr;
}

void probe() {
    AudioComponentDescription identity{fourcc("aufx"), fourcc("HRRV"), fourcc("HRON"), 0, 0};
    const auto component = AudioComponentFindNext(nullptr, &identity);
    require(component && AudioComponentCount(&identity) == 1,
            "installed Revox AU registration is missing or ambiguous");
    InputSource input;
    LoadedUnit unit;
    check(AudioComponentInstanceNew(component, &unit.instance), "create Revox AU");
    AudioStreamBasicDescription format{};
    format.mSampleRate = sample_rate;
    format.mFormatID = kAudioFormatLinearPCM;
    format.mFormatFlags = kAudioFormatFlagsNativeFloatPacked | kAudioFormatFlagIsNonInterleaved;
    format.mBytesPerPacket = format.mBytesPerFrame = sizeof(float);
    format.mFramesPerPacket = format.mChannelsPerFrame = 1;
    format.mBitsPerChannel = 8 * sizeof(float);
    for (const auto scope : {kAudioUnitScope_Input, kAudioUnitScope_Output})
        check(AudioUnitSetProperty(unit.instance, kAudioUnitProperty_StreamFormat, scope, 0,
                                   &format, sizeof(format)), "set mono format");
    const UInt32 maximum_frames = block_size;
    check(AudioUnitSetProperty(unit.instance, kAudioUnitProperty_MaximumFramesPerSlice,
                               kAudioUnitScope_Global, 0, &maximum_frames, sizeof(maximum_frames)),
          "set maximum frames");
    const UInt32 offline = 0;
    check(AudioUnitSetProperty(unit.instance, kAudioUnitProperty_OfflineRender,
                               kAudioUnitScope_Global, 0, &offline, sizeof(offline)),
          "set realtime rendering");
    AURenderCallbackStruct callback{InputSource::render, &input};
    check(AudioUnitSetProperty(unit.instance, kAudioUnitProperty_SetRenderCallback,
                               kAudioUnitScope_Input, 0, &callback, sizeof(callback)),
          "set input callback");
    check(AudioUnitSetParameter(unit.instance, 0, kAudioUnitScope_Global, 0, 3.0f, 0),
          "set +3 semitones");
    check(AudioUnitSetParameter(unit.instance, 1, kAudioUnitScope_Global, 0, 50.0f, 0),
          "set +50 cents");
    check(AudioUnitInitialize(unit.instance), "initialize Revox AU");
    unit.initialized = true;
    Float64 latency_seconds = 0.0;
    UInt32 latency_bytes = sizeof(latency_seconds);
    check(AudioUnitGetProperty(unit.instance, kAudioUnitProperty_Latency,
                               kAudioUnitScope_Global, 0, &latency_seconds, &latency_bytes),
          "get latency");
    require(std::isfinite(latency_seconds) && latency_seconds > 0.0
                && latency_seconds < double(sample_count) / sample_rate,
            "Revox AU returned an invalid latency");
    const auto latency = static_cast<uint32_t>(std::llround(latency_seconds * sample_rate));
    input.gate_position = latency;
    std::array<float, block_size> output{};
    for (size_t start = 0; start < latency;) {
        const auto frames = static_cast<UInt32>(std::min(size_t(block_size), latency - start));
        check(render_block(unit.instance, start, frames, output.data()), "render preroll");
        require(std::all_of(output.begin(), output.begin() + frames,
                            [](float sample) { return sample == 0.0f; }),
                "Revox AU emitted audio before its reported latency");
        start += frames;
    }

    std::promise<void> reset_started;
    auto started = reset_started.get_future();
    std::promise<OSStatus> reset_result;
    auto reset_done = reset_result.get_future();
    OSStatus render_status = noErr;
    std::thread render([&] {
        render_status = render_block(unit.instance, latency, block_size, output.data());
    });
    bool gate_entered;
    {
        std::unique_lock<std::mutex> lock(input.mutex);
        gate_entered = input.condition.wait_for(lock, 5s, [&input] { return input.entered; });
    }
    std::thread reset;
    bool reset_was_started = false;
    bool reset_while_render_active = false;
    if (gate_entered) {
        reset = std::thread([&] {
            reset_started.set_value();
            reset_result.set_value(AudioUnitReset(unit.instance, kAudioUnitScope_Global, 0));
        });
        reset_was_started = started.wait_for(5s) == std::future_status::ready;
        if (reset_was_started)
            reset_while_render_active = reset_done.wait_for(250ms) == std::future_status::ready;
    }
    // Release and join before asserting so even the pre-fix failure cleans up.
    input.release();
    render.join();
    if (reset.joinable()) reset.join();
    require(gate_entered, "Revox AU did not pull the gated Wet input");
    require(reset_was_started, "host reset thread did not start");
    require(!reset_while_render_active, "Revox AU reset completed while its render was active");
    check(render_status, "complete active render");
    check(reset_done.get(), "complete deferred reset");
    require(std::all_of(output.begin(), output.end(), [](float sample) { return std::isfinite(sample); }),
            "Revox AU returned invalid audio after releasing the host input gate");

    // The reset must also start a usable new timeline with fresh preroll and
    // shifted Wet audio. The shared pitch assertion rejects a Dry output path.
    std::vector<float> rendered(sample_count);
    for (size_t start = 0; start < sample_count; start += block_size) {
        const auto frames = static_cast<UInt32>(std::min(size_t(block_size), sample_count - start));
        check(render_block(unit.instance, start, frames, rendered.data() + start),
              "render new timeline after reset");
    }
    revox_audio::verify_audio(rendered, latency, "AUv2 concurrent reset");
}
} // namespace

int main(int argc, char**) {
    if (argc != 1) return 2;
    CompletionDeadline deadline;
    try {
        probe();
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "Revox AU reset probe failed: %s\n", error.what());
        return 1;
    }
}
