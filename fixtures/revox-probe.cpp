// Proves the packaged Revox binary runs its embedded models through the VST3 ABI.
#include "pluginterfaces/base/ipluginbase.h"
#include "pluginterfaces/vst/ivstaudioprocessor.h"
#include "pluginterfaces/vst/ivstparameterchanges.h"
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <string>
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

void require(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

void check(tresult result, const char* operation) {
    if (result != kResultOk)
        throw std::runtime_error(std::string(operation) + ": " + std::to_string(result));
}

// The two parameter queues are borrowed for one synchronous process() call.
// They never escape the probe's stack and are not managed by COM ownership.
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
    // ID 0: +3 in [-12, 12]. ID 1: +50 in [-100, 100].
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

constexpr double sample_rate = 48'000.0;
constexpr int32 block_size = 257;
constexpr size_t sample_count = 192'000;
constexpr double pi = 3.14159265358979323846;

void verify_audio(const std::vector<float>& output, uint32 latency) {
    require(latency == 71'168, "packaged latency differs from 1480 ms + 128 samples");
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
    std::printf("Packaged Revox: latency=%u samples, pitch=%.3f Hz, expected=%.3f Hz, error=%.2f cents, rms=%.5f\n",
                latency, measured, expected, cents, rms);
    // An initialization failure falls back to 220 Hz delayed dry audio and fails
    // this owned DSP assertion, even if the factory and parameter ABI still work.
    require(std::abs(cents) < 50.0, "packaged Revox did not apply the combined +3.5 semitone shift");
}

void probe(const char* path) {
    LoadedPlugin plugin;
    plugin.load(path);
    auto mono = SpeakerArr::kMono;
    check(plugin.processor->setBusArrangements(&mono, 1, &mono, 1), "setBusArrangements");
    check(plugin.component->activateBus(kAudio, kInput, 0, true), "activate input");
    check(plugin.component->activateBus(kAudio, kOutput, 0, true), "activate output");
    ProcessSetup setup{kOffline, kSample32, block_size, sample_rate};
    check(plugin.processor->setupProcessing(setup), "setupProcessing");
    check(plugin.component->setActive(true), "setActive");
    plugin.active = true;
    check(plugin.processor->setProcessing(true), "setProcessing");
    plugin.processing = true;
    // Truce prepares the Rust DSP on activation, after setupProcessing records
    // the configuration. Query the resulting delay once that transition ends.
    const uint32 latency = plugin.processor->getLatencySamples();

    PitchChanges pitch;
    std::array<float, block_size> input{};
    std::array<float, block_size> output{};
    std::vector<float> rendered(sample_count);
    float* input_channel = input.data();
    float* output_channel = output.data();
    AudioBusBuffers input_bus, output_bus;
    input_bus.numChannels = output_bus.numChannels = 1;
    input_bus.channelBuffers32 = &input_channel;
    output_bus.channelBuffers32 = &output_channel;
    ProcessData data;
    data.processMode = kOffline;
    data.symbolicSampleSize = kSample32;
    data.numInputs = data.numOutputs = 1;
    data.inputs = &input_bus;
    data.outputs = &output_bus;
    for (size_t start = 0; start < sample_count; start += block_size) {
        data.numSamples = static_cast<int32>(std::min(size_t(block_size), sample_count - start));
        data.inputParameterChanges = start == 0 ? &pitch : nullptr;
        input_bus.silenceFlags = output_bus.silenceFlags = 0;
        for (int32 index = 0; index < data.numSamples; ++index) {
            const double phase = 2.0 * pi * 220.0 * double(start + index) / sample_rate;
            input[index] = static_cast<float>(0.15 * std::sin(phase) + 0.07 * std::sin(2.0 * phase)
                                             + 0.04 * std::sin(3.0 * phase));
        }
        check(plugin.processor->process(data), "process");
        std::copy_n(output.begin(), data.numSamples, rendered.begin() + start);
    }
    verify_audio(rendered, latency);
}
} // namespace

int main(int argc, char** argv) {
    if (argc != 2) {
        std::fprintf(stderr, "usage: revox-probe <packaged VST3 binary>\n");
        return 2;
    }
    try {
        probe(argv[1]);
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "Revox packaged DSP probe failed: %s\n", error.what());
        return 1;
    }
}
