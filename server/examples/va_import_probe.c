// Aqua phase 3C — Milestone 2 prototype (reference sequence).
//
// Proves the GPU-native path WITHOUT a CPU readback:
//
//   dmabuf (DRM PRIME fd) --> VA-API surface --> VPP (ARGB->NV12) --> encode
//
//   Stage A: import a LINEAR ARGB8888 dmabuf into a VA surface via
//            VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2. Handle-based: 0 CPU copies.
//   Stage B: VAProcPipeline converts ARGB -> NV12 on the GPU.
//   Stage C: VA-API HEVC encode of the NV12 surface (reference; Mesa's HEVC
//            encoder is immature and needs app-supplied packed headers, so the
//            production encoder is done in-process with libavcodec, reusing the
//            ffmpeg hevc_vaapi encoder — see notes at the bottom).
//
// Build (Linux box, userland sysroot):
//   source ~/.aqua-p2p/sysroot-env.sh
//   S=$HOME/.local/sysroot
//   gcc -I$S/usr/include -I$S/usr/include/libdrm \
//       server/examples/va_import_probe.c -o /tmp/va_probe \
//       -L$S/usr/lib/x86_64-linux-gnu -lgbm -ldrm \
//       /usr/lib/x86_64-linux-gnu/libva.so.2 /usr/lib/x86_64-linux-gnu/libva-drm.so.2

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>

#include <va/va.h>
#include <va/va_drm.h>
#include <va/va_drmcommon.h>
#include <va/va_vpp.h>
#include <va/va_enc_hevc.h>
#include <gbm.h>
#include <xf86drm.h>
#include <drm_fourcc.h>

#define PROBE_W 256
#define PROBE_H 256
#define DRM_NODE "/dev/dri/renderD128"

static int expect_ok(const char *what, VAStatus st) {
    if (st != VA_STATUS_SUCCESS) {
        fprintf(stderr, "FAIL  %s: %s (0x%x)\n", what, vaErrorStr(st), st);
        return -1;
    }
    printf("ok    %s\n", what);
    return 0;
}

int main(void) {
    int drm_fd = open(DRM_NODE, O_RDWR | O_CLOEXEC);
    if (drm_fd < 0) { perror("open " DRM_NODE); return 1; }

    VADisplay dpy = vaGetDisplayDRM(drm_fd);
    if (!dpy) { fprintf(stderr, "vaGetDisplayDRM failed\n"); return 1; }

    int major = 0, minor = 0;
    if (expect_ok("vaInitialize", vaInitialize(dpy, &major, &minor))) return 1;
    printf("      VA %d.%d  vendor=%s\n", major, minor, vaQueryVendorString(dpy));

    struct gbm_device *gbm = gbm_create_device(drm_fd);
    if (!gbm) { fprintf(stderr, "gbm_create_device failed\n"); return 1; }

    struct gbm_bo *bo = gbm_bo_create(
        gbm, PROBE_W, PROBE_H, GBM_FORMAT_ARGB8888,
        GBM_BO_USE_LINEAR | GBM_BO_USE_RENDERING);
    if (!bo) { fprintf(stderr, "gbm_bo_create failed (LINEAR ARGB8888)\n"); return 1; }

    int prime_fd = gbm_bo_get_fd(bo);
    uint32_t stride = gbm_bo_get_stride(bo);
    uint32_t height = gbm_bo_get_height(bo);
    printf("      gbm bo %ux%u stride=%u prime_fd=%d\n",
           gbm_bo_get_width(bo), height, stride, prime_fd);

    // Test-content generation. NOTE: this write is not part of the import path;
    // the import itself never maps the buffer.
    uint32_t map_stride = 0;
    void *map_data = NULL;
    uint32_t *pixels = gbm_bo_map(bo, 0, 0, PROBE_W, PROBE_H,
                                  GBM_BO_TRANSFER_WRITE, &map_stride, &map_data);
    if (!pixels) { fprintf(stderr, "gbm_bo_map failed\n"); return 1; }
    for (uint32_t y = 0; y < height; ++y) {
        for (uint32_t x = 0; x < PROBE_W; ++x) {
            uint8_t r = (uint8_t)(x * 255 / PROBE_W);
            uint8_t g = (uint8_t)(y * 255 / height);
            pixels[y * (map_stride / 4) + x] =
                0xff000000u | ((uint32_t)r << 16) | ((uint32_t)g << 8) | 0x80u;
        }
    }
    gbm_bo_unmap(bo, map_data);

    // --- Stage A: import dmabuf fd -> VA surface (no CPU copy) --------------
    VADRMPRIMESurfaceDescriptor desc;
    memset(&desc, 0, sizeof(desc));
    desc.fourcc = VA_FOURCC_ARGB;
    desc.width = PROBE_W;
    desc.height = PROBE_H;
    desc.num_objects = 1;
    desc.objects[0].fd = prime_fd;
    desc.objects[0].size = stride * height;
    desc.objects[0].drm_format_modifier = DRM_FORMAT_MOD_LINEAR;
    desc.num_layers = 1;
    desc.layers[0].drm_format = DRM_FORMAT_ARGB8888;
    desc.layers[0].num_planes = 1;
    desc.layers[0].object_index[0] = 0;
    desc.layers[0].offset[0] = 0;
    desc.layers[0].pitch[0] = stride;

    VASurfaceAttrib attrs[2];
    memset(attrs, 0, sizeof(attrs));
    attrs[0].type = VASurfaceAttribMemoryType;
    attrs[0].flags = VA_SURFACE_ATTRIB_SETTABLE;
    attrs[0].value.type = VAGenericValueTypeInteger;
    attrs[0].value.value.i = VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2;
    attrs[1].type = VASurfaceAttribExternalBufferDescriptor;
    attrs[1].flags = VA_SURFACE_ATTRIB_SETTABLE;
    attrs[1].value.type = VAGenericValueTypePointer;
    attrs[1].value.value.p = &desc;

    VASurfaceID surface = VA_INVALID_SURFACE;
    VAStatus st = vaCreateSurfaces(dpy, VA_RT_FORMAT_RGB32, PROBE_W, PROBE_H,
                                   &surface, 1, attrs, 2);
    if (expect_ok("vaCreateSurfaces(DRM_PRIME_2 import)", st)) {
        fprintf(stderr, "IMPORT_REJECTED\n");
        vaTerminate(dpy);
        return 2;
    }
    printf("IMPORT_OK surface=%u fourcc=AR24 modifier=LINEAR planes=1 stride=%u\n",
           surface, stride);

    // --- Stage B: GPU color convert ARGB -> NV12 (VAProcPipeline) -----------
    VASurfaceID nv12 = VA_INVALID_SURFACE;
    st = vaCreateSurfaces(dpy, VA_RT_FORMAT_YUV420, PROBE_W, PROBE_H, &nv12, 1, NULL, 0);
    expect_ok("vaCreateSurfaces(NV12 output)", st);

    VAConfigAttrib vpp_attr = { VAConfigAttribRTFormat, VA_RT_FORMAT_YUV420 };
    VAConfigID vpp_cfg = VA_INVALID_ID;
    VAContextID vpp_ctx = VA_INVALID_ID;
    if (expect_ok("vaCreateConfig(VideoProc)",
                  vaCreateConfig(dpy, VAProfileNone, VAEntrypointVideoProc, &vpp_attr, 1, &vpp_cfg)) == 0 &&
        expect_ok("vaCreateContext(VPP)",
                  vaCreateContext(dpy, vpp_cfg, PROBE_W, PROBE_H, VA_PROGRESSIVE, &nv12, 1, &vpp_ctx)) == 0) {
        VAProcPipelineParameterBuffer pp;
        memset(&pp, 0, sizeof(pp));
        pp.surface = surface;
        pp.surface_color_standard = VAProcColorStandardNone;
        pp.output_background_color = 0xff000000u;

        VABufferID pp_buf = VA_INVALID_ID;
        if (expect_ok("vaCreateBuffer(VAProcPipelineParameterBuffer)",
                      vaCreateBuffer(dpy, vpp_ctx, VAProcPipelineParameterBufferType,
                                     sizeof(pp), 1, &pp, &pp_buf)) == 0) {
            vaBeginPicture(dpy, vpp_ctx, nv12);
            vaRenderPicture(dpy, vpp_ctx, &pp_buf, 1);
            vaEndPicture(dpy, vpp_ctx);
            if (expect_ok("vaSyncSurface(VPP)", vaSyncSurface(dpy, nv12)) == 0) {
                printf("VPP_OK surface(ARGB)->surface(NV12) on GPU\n");
            }
        }
    }
    if (vpp_ctx != VA_INVALID_ID) vaDestroyContext(dpy, vpp_ctx);
    if (vpp_cfg != VA_INVALID_ID) vaDestroyConfig(dpy, vpp_cfg);
    vaDestroySurfaces(dpy, &nv12, 1);
    vaDestroySurfaces(dpy, &surface, 1);
    vaTerminate(dpy);
    gbm_bo_destroy(bo);
    gbm_device_destroy(gbm);
    close(drm_fd);

    // ---------------------------------------------------------------------
    // NOTE (integration): Stage C (HEVC encode) is NOT hand-rolled here.
    // Mesa's VA-API HEVC encoder expects app-supplied packed headers
    // (VPS/SPS/PPS) and produced an invalid 49-byte stream when driven
    // naively; Mesa's H.264 encoder also crashed. The production encoder must
    // reuse ffmpeg's working hevc_vaapi path **in-process** (libavcodec + a
    // VAAPI hwdevice sharing this VADisplay) so the imported/VPP NV12 surface
    // is encoded with no CPU copy. libavcodec dev headers+libs are present in
    // the sysroot (usr/include/x86_64-linux-gnu/libavcodec).
    // ---------------------------------------------------------------------
    (void)expect_ok;
    return 0;
}
