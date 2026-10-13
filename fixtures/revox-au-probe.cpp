// Proves the installed Revox AU runs its bundled runtime and embedded models.
#include <AudioToolbox/AudioToolbox.h>
#include "revox-audio-probe.h"
#include <array>
#include <limits>
#include <string>

namespace {
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

struct LoadedUnit {
    AudioComponentInstance instance = nullptr;
    bool initialized = false;

    ~LoadedUnit() {
        if (initialized) AudioUnitUninitialize(instance);
        if (instance) AudioComponentInstanceDispose(instance);
    }
};

struct InputSource {
    std::array<float, block_size> samples{};

    // All storage outlives the unit; this callback neither allocates nor throws.
    static OSStatus render(void* context, AudioUnitRenderActionFlags* flags,
                           const AudioTimeStamp* time, UInt32 bus, UInt32 frames,
                           AudioBufferList* data) noexcept {
        if (!context || !time || !data || bus != 0 || frames > block_size
            || data->mNumberBuffers != 1 || data->mBuffers[0].mNumberChannels != 1
            || !(time->mFlags & kAudioTimeStampSampleTimeValid)
            || !std::isfinite(time->mSampleTime) || time->mSampleTime < 0.0
            || time->mSampleTime >= double(sample_count)
            || std::floor(time->mSampleTime) != time->mSampleTime)
            return kAudioUnitErr_InvalidParameter;
        const auto start = static_cast<size_t>(time->mSampleTime);
        if (frames > sample_count - start) return kAudioUnitErr_InvalidParameter;
        auto& source = *static_cast<InputSource*>(context);
        for (UInt32 index = 0; index < frames; ++index)
            source.samples[index] = revox_audio::input_sample(start + index);
        data->mBuffers[0].mData = source.samples.data();
        data->mBuffers[0].mDataByteSize = frames * sizeof(float);
        if (flags) *flags &= ~kAudioUnitRenderAction_OutputIsSilence;
        return noErr;
    }
};

void probe() {
    AudioComponentDescription identity{fourcc("aufx"), fourcc("HRRV"), fourcc("HRON"), 0, 0};
    const auto component = AudioComponentFindNext(nullptr, &identity);
    require(component != nullptr, "installed Heron Revox AU registration is missing");
    require(AudioComponentCount(&identity) == 1, "installed Heron Revox AU registration is ambiguous");
    InputSource input;
    LoadedUnit unit;
    check(AudioComponentInstanceNew(component, &unit.instance), "AudioComponentInstanceNew");
    require(unit.instance != nullptr, "installed Revox AU returned no instance");

    AudioStreamBasicDescription stream{};
    stream.mSampleRate = sample_rate;
    stream.mFormatID = kAudioFormatLinearPCM;
    stream.mFormatFlags = kAudioFormatFlagsNativeFloatPacked | kAudioFormatFlagIsNonInterleaved;
    stream.mBytesPerPacket = stream.mBytesPerFrame = sizeof(float);
    stream.mFramesPerPacket = stream.mChannelsPerFrame = 1;
    stream.mBitsPerChannel = 8 * sizeof(float);
    for (const auto scope : {kAudioUnitScope_Input, kAudioUnitScope_Output})
        check(AudioUnitSetProperty(unit.instance, kAudioUnitProperty_StreamFormat, scope, 0,
                                   &stream, sizeof(stream)), "set mono stream format");
    const UInt32 maximum_frames = block_size;
    check(AudioUnitSetProperty(unit.instance, kAudioUnitProperty_MaximumFramesPerSlice,
                               kAudioUnitScope_Global, 0, &maximum_frames, sizeof(maximum_frames)),
          "set maximum frames per slice");
    const UInt32 offline = 1;
    check(AudioUnitSetProperty(unit.instance, kAudioUnitProperty_OfflineRender,
                               kAudioUnitScope_Global, 0, &offline, sizeof(offline)),
          "enable offline rendering");
    AURenderCallbackStruct callback{InputSource::render, &input};
    check(AudioUnitSetProperty(unit.instance, kAudioUnitProperty_SetRenderCallback,
                               kAudioUnitScope_Input, 0, &callback, sizeof(callback)),
          "set input callback");
    // AU uses plain values: ID 0 is semitones, ID 1 is cents.
    check(AudioUnitSetParameter(unit.instance, 0, kAudioUnitScope_Global, 0, 3.0f, 0),
          "set +3 semitones");
    check(AudioUnitSetParameter(unit.instance, 1, kAudioUnitScope_Global, 0, 50.0f, 0),
          "set +50 cents");
    check(AudioUnitInitialize(unit.instance), "AudioUnitInitialize");
    unit.initialized = true;

    Float64 latency_seconds = 0.0;
    UInt32 latency_bytes = sizeof(latency_seconds);
    check(AudioUnitGetProperty(unit.instance, kAudioUnitProperty_Latency,
                               kAudioUnitScope_Global, 0, &latency_seconds, &latency_bytes),
          "get latency");
    require(latency_bytes == sizeof(latency_seconds) && std::isfinite(latency_seconds)
                && latency_seconds >= 0.0 && latency_seconds < double(sample_count) / sample_rate,
            "installed Revox AU returned an invalid latency");
    const auto latency = static_cast<uint32_t>(std::llround(latency_seconds * sample_rate));

    std::array<float, block_size> output{};
    std::vector<float> rendered(sample_count);
    for (size_t start = 0; start < sample_count; start += block_size) {
        const auto frames = static_cast<UInt32>(std::min(size_t(block_size), sample_count - start));
        // NaNs make a successful render that does not write output fail the audio check.
        output.fill(std::numeric_limits<float>::quiet_NaN());
        AudioBufferList data{};
        data.mNumberBuffers = 1;
        data.mBuffers[0] = {1, frames * UInt32(sizeof(float)), output.data()};
        AudioTimeStamp time{};
        time.mSampleTime = double(start);
        time.mFlags = kAudioTimeStampSampleTimeValid;
        AudioUnitRenderActionFlags flags = 0;
        check(AudioUnitRender(unit.instance, &flags, &time, 0, frames, &data), "AudioUnitRender");
        require(data.mNumberBuffers == 1 && data.mBuffers[0].mNumberChannels == 1
                    && data.mBuffers[0].mData && data.mBuffers[0].mDataByteSize >= frames * sizeof(float),
                "installed Revox AU returned an invalid output buffer");
        std::copy_n(static_cast<const float*>(data.mBuffers[0].mData), frames, rendered.begin() + start);
    }
    revox_audio::verify_audio(rendered, latency, "AUv2");
}
} // namespace

int main(int argc, char**) {
    if (argc != 1) {
        std::fprintf(stderr, "usage: revox-au-probe (installed aufx:HRRV:HRON component)\n");
        return 2;
    }
    try {
        probe();
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "Revox packaged AU DSP probe failed: %s\n", error.what());
        return 1;
    }
}
