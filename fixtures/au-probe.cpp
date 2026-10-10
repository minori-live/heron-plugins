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
        const auto registeredCount = AudioComponentCount(&expected);
        if (!component) {
            std::fprintf(stderr, "AU registration missing: %s:%s:%s count=%u\n", argv[offset], argv[offset + 1], argv[offset + 2], registeredCount);
            return 3;
        }
        if (registeredCount != 1 || AudioComponentFindNext(component, &expected)) {
            std::fprintf(stderr, "AU registration duplicate: %s:%s:%s count=%u\n", argv[offset], argv[offset + 1], argv[offset + 2], registeredCount);
            return 8;
        }
        AudioComponentDescription actual{};
        auto status = AudioComponentGetDescription(component, &actual);
        if (status != noErr || actual.componentType != expected.componentType || actual.componentSubType != expected.componentSubType || actual.componentManufacturer != expected.componentManufacturer) {
            std::fprintf(stderr, "AU identity mismatch: %s:%s:%s status=%d\n", argv[offset], argv[offset + 1], argv[offset + 2], status);
            return 4;
        }
        UInt32 version = 0;
        const auto expectedVersion = std::strtoul(argv[offset + 3], nullptr, 10);
        status = AudioComponentGetVersion(component, &version);
        if (status != noErr || version != expectedVersion) {
            std::fprintf(stderr, "AU version mismatch: %s:%s:%s expected=%lu actual=%u status=%d\n", argv[offset], argv[offset + 1], argv[offset + 2], expectedVersion, version, status);
            return 5;
        }
        status = AudioComponentInstanceNew(component, &instances[i]);
        if (status != noErr || !instances[i]) {
            std::fprintf(stderr, "AU instance failed: %s:%s:%s status=%d\n", argv[offset], argv[offset + 1], argv[offset + 2], status);
            return 6;
        }
    }
    // Keep the entire suite loaded together to expose AU/Objective-C registration collisions.
    for (auto instance : instances)
        if (AudioComponentInstanceDispose(instance) != noErr) return 7;
    std::printf("All %d AU identities, versions and simultaneous instances verified\n", count);
    return 0;
}
