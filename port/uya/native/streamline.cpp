// NVIDIA Streamline (DLSS Frame Generation, Reflex, PC Latency) for the port, behind a C interface (src/streamline.rs). Streamline's
// headers are C++ (versioned structures with default members), so the structures are filled here with the SDK's own definitions
// and the Rust side passes plain values. sl.interposer.dll is loaded at run time, never linked: with FRAME GENERATION off the
// game does not load Streamline at all.
//
// Every function returns an sl::Result (0 is eOk) or, for a failure of this layer's own, a negative number.
#define NOMINMAX
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <string>
#include <sl.h>
#include <sl_consts.h>
#include <sl_dlss_g.h>
#include <sl_reflex.h>
#include <sl_pcl.h>

namespace {
typedef void (*Log)(uint32_t type, const char* message);
Log g_log = nullptr;
HMODULE g_module = nullptr;
PFun_slInit* f_init;
PFun_slShutdown* f_shutdown;
PFun_slIsFeatureSupported* f_supported;
PFun_slGetFeatureFunction* f_function;
PFun_slGetNewFrameToken* f_token;
PFun_slSetTagForFrame* f_tag;
PFun_slSetConstants* f_constants;
PFun_slDLSSGSetOptions* f_options;
PFun_slDLSSGGetState* f_state;
PFun_slReflexSetOptions* f_reflex_options;
PFun_slReflexSleep* f_sleep;
PFun_slPCLSetMarker* f_marker;

void log_message(sl::LogType type, const char* message) {
    if (g_log) g_log((uint32_t)type, message);
}

template <class F> bool load(F*& f, const char* name) {
    f = reinterpret_cast<F*>(GetProcAddress(g_module, name));
    return f != nullptr;
}

const sl::ViewportHandle kViewport(0);
const float kIdentity[16] = { 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1 };

sl::FrameToken* token(uint32_t frame) {
    sl::FrameToken* t = nullptr;
    if (f_token(t, &frame) != sl::Result::eOk) return nullptr;
    return t;
}

sl::float4x4 matrix(const float* m) {
    sl::float4x4 r;
    for (int i = 0; i < 4; i++) r[i] = sl::float4(m[i * 4], m[i * 4 + 1], m[i * 4 + 2], m[i * 4 + 3]);
    return r;
}
}

extern "C" {

struct UyaImage {
    void* image;
    void* view;
    uint32_t layout, width, height, format, usage, flags;
};

struct UyaConstants {
    // Row-major, as Streamline wants them, without the jitter.
    float view_to_clip[16], clip_to_view[16], clip_to_prev[16], prev_to_clip[16];
    float jitter[2], mvec_scale[2], position[3], up[3], right[3], forward[3];
    float near_plane, far_plane, fov, aspect;
    uint32_t reset;
};

// Loads `dir`\sl.interposer.dll and initialises Streamline for DLSS-G and Reflex (PC Latency is always loaded) on Vulkan. Streamline's
// own over-the-air updates are off: nothing is downloaded.
int uya_sl_init(const wchar_t* dir, const wchar_t* logs, Log log, const char* project) {
    g_log = log;
    std::wstring path = std::wstring(dir) + L"\\sl.interposer.dll";
    g_module = LoadLibraryW(path.c_str());
    if (!g_module) return -(int)GetLastError();
    if (!(load(f_init, "slInit") && load(f_shutdown, "slShutdown") && load(f_supported, "slIsFeatureSupported") && load(f_function, "slGetFeatureFunction") && load(f_token, "slGetNewFrameToken") && load(f_tag, "slSetTagForFrame") && load(f_constants, "slSetConstants"))) return -1;
    const sl::Feature features[] = { sl::kFeatureDLSS_G, sl::kFeatureReflex, sl::kFeaturePCL };
    const wchar_t* plugins[] = { dir };
    sl::Preferences pref{};
    pref.showConsole = false;
    pref.logLevel = sl::LogLevel::eDefault;
    pref.pathsToPlugins = plugins;
    pref.numPathsToPlugins = 1;
    pref.pathToLogsAndData = logs;
    pref.logMessageCallback = log_message;
    pref.flags = sl::PreferenceFlags::eDisableCLStateTracking | sl::PreferenceFlags::eUseFrameBasedResourceTagging;
    pref.featuresToLoad = features;
    pref.numFeaturesToLoad = 3;
    pref.engine = sl::EngineType::eCustom;
    pref.engineVersion = "1.0";
    pref.projectId = project;
    pref.renderAPI = sl::RenderAPI::eVulkan;
    return (int)f_init(pref, sl::kSDKVersion);
}

// Whether the GPU `physical_device` (a VkPhysicalDevice) runs DLSS-G, Reflex and PC Latency: 0, else the first failure, `which` its
// feature.
int uya_sl_supported(void* physical_device, uint32_t* which) {
    sl::AdapterInfo adapter{};
    adapter.vkPhysicalDevice = physical_device;
    for (sl::Feature f : { sl::kFeatureDLSS_G, sl::kFeatureReflex, sl::kFeaturePCL }) {
        sl::Result r = f_supported(f, adapter);
        if (r != sl::Result::eOk) {
            *which = f;
            return (int)r;
        }
    }
    return 0;
}

// The feature functions, once the device exists (Streamline makes them with it).
int uya_sl_ready() {
    void* f = nullptr;
    sl::Result r;
    if ((r = f_function(sl::kFeatureDLSS_G, "slDLSSGSetOptions", f)) != sl::Result::eOk) return (int)r;
    f_options = reinterpret_cast<PFun_slDLSSGSetOptions*>(f);
    if ((r = f_function(sl::kFeatureDLSS_G, "slDLSSGGetState", f)) != sl::Result::eOk) return (int)r;
    f_state = reinterpret_cast<PFun_slDLSSGGetState*>(f);
    if ((r = f_function(sl::kFeatureReflex, "slReflexSetOptions", f)) != sl::Result::eOk) return (int)r;
    f_reflex_options = reinterpret_cast<PFun_slReflexSetOptions*>(f);
    if ((r = f_function(sl::kFeatureReflex, "slReflexSleep", f)) != sl::Result::eOk) return (int)r;
    f_sleep = reinterpret_cast<PFun_slReflexSleep*>(f);
    if ((r = f_function(sl::kFeaturePCL, "slPCLSetMarker", f)) != sl::Result::eOk) return (int)r;
    f_marker = reinterpret_cast<PFun_slPCLSetMarker*>(f);
    return 0;
}

// DLSS-G's state: the most frames it can generate per real one, its status (0 ok), the frames it presented since the last call, the
// smallest picture side it runs at.
int uya_sl_state(uint32_t* max_generated, uint32_t* status, uint32_t* presented, uint32_t* min_side) {
    sl::DLSSGState state{};
    sl::Result r = f_state(kViewport, state, nullptr);
    *max_generated = state.numFramesToGenerateMax;
    *status = (uint32_t)state.status;
    *presented = state.numFramesActuallyPresented;
    *min_side = state.minWidthOrHeight;
    return (int)r;
}

// Reflex on (low latency) or off.
int uya_sl_reflex(int on) {
    sl::ReflexOptions o{};
    o.mode = on ? sl::ReflexMode::eLowLatency : sl::ReflexMode::eOff;
    return (int)f_reflex_options(o);
}

int uya_sl_sleep(uint32_t frame) {
    sl::FrameToken* t = token(frame);
    return t ? (int)f_sleep(*t) : -2;
}

// A PC Latency marker (`sl::PCLMarker`) of frame `frame`.
int uya_sl_marker(uint32_t marker, uint32_t frame) {
    sl::FrameToken* t = token(frame);
    return t ? (int)f_marker((sl::PCLMarker)marker, *t) : -2;
}

// DLSS-G on with `generated` frames per real one (0: off), for the next present.
int uya_sl_options(uint32_t generated, uint32_t back_buffers) {
    sl::DLSSGOptions o{};
    o.mode = generated ? sl::DLSSGMode::eOn : sl::DLSSGMode::eOff;
    o.numFramesToGenerate = generated ? generated : 1;
    o.flags = sl::DLSSGFlags::eRetainResourcesWhenOff;
    o.numBackBuffers = back_buffers;
    return (int)f_options(kViewport, o);
}

// Frame `frame`'s constants.
int uya_sl_constants(uint32_t frame, const UyaConstants* c) {
    sl::FrameToken* t = token(frame);
    if (!t) return -2;
    sl::Constants k{};
    k.cameraViewToClip = matrix(c->view_to_clip);
    k.clipToCameraView = matrix(c->clip_to_view);
    k.clipToLensClip = matrix(kIdentity);
    k.clipToPrevClip = matrix(c->clip_to_prev);
    k.prevClipToClip = matrix(c->prev_to_clip);
    k.jitterOffset = sl::float2(c->jitter[0], c->jitter[1]);
    k.mvecScale = sl::float2(c->mvec_scale[0], c->mvec_scale[1]);
    k.cameraPinholeOffset = sl::float2(0, 0);
    k.cameraPos = sl::float3(c->position[0], c->position[1], c->position[2]);
    k.cameraUp = sl::float3(c->up[0], c->up[1], c->up[2]);
    k.cameraRight = sl::float3(c->right[0], c->right[1], c->right[2]);
    k.cameraFwd = sl::float3(c->forward[0], c->forward[1], c->forward[2]);
    k.cameraNear = c->near_plane;
    k.cameraFar = c->far_plane;
    k.cameraFOV = c->fov;
    k.cameraAspectRatio = c->aspect;
    k.depthInverted = sl::Boolean::eTrue;
    k.cameraMotionIncluded = sl::Boolean::eTrue;
    k.motionVectors3D = sl::Boolean::eFalse;
    k.reset = c->reset ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    k.orthographicProjection = sl::Boolean::eFalse;
    k.motionVectorsDilated = sl::Boolean::eFalse;
    k.motionVectorsJittered = sl::Boolean::eFalse;
    return (int)f_constants(k, *t, kViewport);
}

// Frame `frame`'s resources: depth, motion vectors and the picture before the HUD (null: none), each copied by Streamline into the
// command buffer `cmd` (a VkCommandBuffer, recording) as they are there, in the layouts given: they are overdrawn by the next picture
// before this one is presented.
int uya_sl_tag(uint32_t frame, void* cmd, const UyaImage* depth, const UyaImage* motion, const UyaImage* hudless) {
    sl::FrameToken* t = token(frame);
    if (!t) return -2;
    sl::Resource r[3] = {
        sl::Resource(sl::ResourceType::eTex2d, nullptr, nullptr, nullptr, 0),
        sl::Resource(sl::ResourceType::eTex2d, nullptr, nullptr, nullptr, 0),
        sl::Resource(sl::ResourceType::eTex2d, nullptr, nullptr, nullptr, 0),
    };
    const UyaImage* images[3] = { depth, motion, hudless };
    const sl::BufferType types[3] = { sl::kBufferTypeDepth, sl::kBufferTypeMotionVectors, sl::kBufferTypeHUDLessColor };
    sl::ResourceTag tags[3] = {
        sl::ResourceTag(nullptr, types[0], sl::eOnlyValidNow), sl::ResourceTag(nullptr, types[1], sl::eOnlyValidNow), sl::ResourceTag(nullptr, types[2], sl::eOnlyValidNow),
    };
    for (int i = 0; i < 3; i++) {
        const UyaImage* im = images[i];
        if (!im) continue;
        r[i].native = im->image;
        r[i].view = im->view;
        r[i].state = im->layout;
        r[i].width = im->width;
        r[i].height = im->height;
        r[i].nativeFormat = im->format;
        r[i].mipLevels = 1;
        r[i].arrayLayers = 1;
        r[i].flags = im->flags;
        r[i].usage = im->usage;
        tags[i].resource = &r[i];
        tags[i].extent = sl::Extent{ 0, 0, im->width, im->height };
    }
    return (int)f_tag(*t, kViewport, tags, 3, cmd);
}

int uya_sl_shutdown() {
    return f_shutdown ? (int)f_shutdown() : 0;
}

}
