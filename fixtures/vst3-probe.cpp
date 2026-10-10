// Proves the suite's fixed class identity at the actual published binary boundary.
#include "pluginterfaces/base/ipluginbase.h"
#include <cstdio>
#include <cstring>
#ifdef _WIN32
#include <windows.h>
#else
#include <dlfcn.h>
#endif

int main(int argc, char** argv) {
    if (argc != 3) return 2;
#ifdef _WIN32
    auto module = LoadLibraryA(argv[1]);
    auto entry = module ? GetProcAddress(module, "GetPluginFactory") : nullptr;
#else
    auto module = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    auto entry = module ? dlsym(module, "GetPluginFactory") : nullptr;
    if (!module) std::fprintf(stderr, "%s\n", dlerror());
#endif
    if (!entry) return 3;
    auto factory = reinterpret_cast<Steinberg::IPluginFactory* (*)()>(entry)();
    if (!factory) return 4;
    int matched = 0;
    int processors = 0;
    for (int i = 0; i < factory->countClasses(); ++i) {
        Steinberg::PClassInfo info{};
        if (factory->getClassInfo(i, &info) != 0) return 5;
        if (std::strcmp(info.category, "Audio Module Class") != 0) continue;
        ++processors;
        // Heron uses GUID field order on every OS; Truce exports COM-layout bytes.
        const int order[] = {3,2,1,0,5,4,7,6,8,9,10,11,12,13,14,15};
        char id[33]{};
        for (int j = 0; j < 16; ++j) std::snprintf(id + j * 2, 3, "%02X", static_cast<unsigned char>(info.cid[order[j]]));
        if (std::strcmp(id, argv[2]) == 0) ++matched;
    }
    factory->release();
    if (matched != 1 || processors != 1) return 6;
    std::puts("Published VST3 identity verified");
    return 0;
}
