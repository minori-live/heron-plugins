#include <AudioToolbox/AudioToolbox.h>
#include <cstdlib>
#include <cstring>
#include <vector>
#include <cstdio>

static UInt32 fourcc(const char* text) {
    if (std::strlen(text) != 4) return 0;
    return (UInt32(text[0]) << 24) | (UInt32(text[1]) << 16) | (UInt32(text[2]) << 8) | UInt32(text[3]);
}
int main(int argc, char** argv) {
    if (argc < 5 || (argc - 1) % 4 != 0) return 2;
    const int count = (argc - 1) / 4;
    std::vector<AudioComponentInstance> instances(count);
    for (int i = 0; i < count; ++i) {
        const int offset = 1 + i * 4;
        AudioComponentDescription expected{fourcc(argv[offset]), fourcc(argv[offset + 1]), fourcc(argv[offset + 2]), 0, 0};
        auto component = AudioComponentFindNext(nullptr, &expected);
        if (!component || AudioComponentFindNext(component, &expected)) return 3;
        AudioComponentDescription actual{};
        if (AudioComponentGetDescription(component, &actual) != noErr || actual.componentType != expected.componentType || actual.componentSubType != expected.componentSubType || actual.componentManufacturer != expected.componentManufacturer) return 4;
        UInt32 version = 0;
        if (AudioComponentGetVersion(component, &version) != noErr || version != std::strtoul(argv[offset + 3], nullptr, 10)) return 5;
        if (AudioComponentInstanceNew(component, &instances[i]) != noErr || !instances[i]) return 6;
    }
    // Keep the entire suite loaded together to expose AU/Objective-C registration collisions.
    for (auto instance : instances)
        if (AudioComponentInstanceDispose(instance) != noErr) return 7;
    std::printf("All %d AU identities, versions and simultaneous instances verified\n", count);
    return 0;
}
