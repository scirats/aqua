// Aqua phase 3C — Milestone 2 Stage C (encoder), libavcodec in-process.
//
//   dmabuf --> VA surface (import, 0 CPU copy)
//          --> VPP ARGB->NV12 (GPU)
//          --> hevc_vaapi encode (libavcodec, same VADisplay)
//          --> Annex-B
//
// We deliberately reuse ffmpeg's working `hevc_vaapi` encoder instead of
// hand-rolling libva HEVC parameter sets (which Mesa handles unreliably).
// The trick: create the VAAPI hwdevice with libav, take its `VADisplay`, do our
// import + VPP on that same display, then hand the resulting NV12 `VASurfaceID`
// to libavcodec via `AVFrame.data[3]` with `AV_PIX_FMT_VAAPI`.
//
// Build (Linux box, userland sysroot):
//   source ~/.aqua-p2p/sysroot-env.sh
//   S=$HOME/.local/sysroot
//   gcc -I$S/usr/include -I$S/usr/include/libdrm -I$S/usr/include/x86_64-linux-gnu \
//       server/examples/libav_encode_probe.c -o /tmp/libav_probe \
//       -L$S/usr/lib/x86_64-linux-gnu -lgbm -ldrm -lavcodec -lavutil \
//       /usr/lib/x86_64-linux-gnu/libva.so.2 /usr/lib/x86_64-linux-gnu/libva-drm.so.2

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <fcntl.h>
#include <unistd.h>

#include <va/va.h>
#include <va/va_drmcommon.h>
#include <va/va_vpp.h>
#include <gbm.h>
#include <drm_fourcc.h>

#include <libavcodec/avcodec.h>
#include <libavutil/hwcontext.h>
#include <libavutil/hwcontext_vaapi.h>
#include <libavutil/opt.h>

#define PROBE_W 256
#define PROBE_H 256

#define CHECK(cond, msg) do { if (!(cond)) { fprintf(stderr, "FAIL %s\n", msg); return 1; } } while (0)

int main(void) {
    // --- GBM: allocate a LINEAR ARGB8888 dmabuf -------------------------
    int drm_fd = open("/dev/dri/renderD128", O_RDWR | O_CLOEXEC);
    if (drm_fd < 0) { perror("open renderD128"); return 1; }
    struct gbm_device *gbm = gbm_create_device(drm_fd);
    CHECK(gbm, "gbm_create_device");
    struct gbm_bo *bo = gbm_bo_create(gbm, PROBE_W, PROBE_H, GBM_FORMAT_ARGB8888,
                                      GBM_BO_USE_LINEAR | GBM_BO_USE_RENDERING);
    CHECK(bo, "gbm_bo_create");
    int prime_fd = gbm_bo_get_fd(bo);
    uint32_t stride = gbm_bo_get_stride(bo);

    uint32_t map_stride = 0; void *map_data = NULL;
    uint32_t *pixels = gbm_bo_map(bo, 0, 0, PROBE_W, PROBE_H, GBM_BO_TRANSFER_WRITE, &map_stride, &map_data);
    CHECK(pixels, "gbm_bo_map");
    for (uint32_t y = 0; y < PROBE_H; ++y)
        for (uint32_t x = 0; x < PROBE_W; ++x)
            pixels[y * (map_stride / 4) + x] =
                0xff000000u | ((uint32_t)(x * 255 / PROBE_W) << 16) | ((uint32_t)(y * 255 / PROBE_H) << 8);
    gbm_bo_unmap(bo, map_data);

    // --- libav VAAPI device; reuse its VADisplay for import + VPP -------
    AVBufferRef *device_ref = NULL;
    if (av_hwdevice_ctx_create(&device_ref, AV_HWDEVICE_TYPE_VAAPI, "/dev/dri/renderD128", NULL, 0) < 0) {
        fprintf(stderr, "av_hwdevice_ctx_create failed\n"); return 1;
    }
    AVHWDeviceContext *dev = (AVHWDeviceContext *)device_ref->data;
    AVVAAPIDeviceContext *vac = (AVVAAPIDeviceContext *)dev->hwctx;
    VADisplay dpy = vac->display;
    printf("libav VAAPI device on %s\n", vaQueryVendorString(dpy));

    // NV12 frames context on the same device.
    AVBufferRef *frames_ref = av_hwframe_ctx_alloc(device_ref);
    CHECK(frames_ref, "av_hwframe_ctx_alloc");
    AVHWFramesContext *fc = (AVHWFramesContext *)frames_ref->data;
    fc->format = AV_PIX_FMT_VAAPI;
    fc->sw_format = AV_PIX_FMT_NV12;
    fc->width = PROBE_W;
    fc->height = PROBE_H;
    fc->initial_pool_size = 4;
    CHECK(av_hwframe_ctx_init(frames_ref) >= 0, "av_hwframe_ctx_init");

    // --- Stage A: import dmabuf -> VA surface (0 CPU copies) -----------
    VADRMPRIMESurfaceDescriptor desc;
    memset(&desc, 0, sizeof(desc));
    desc.fourcc = VA_FOURCC_ARGB;
    desc.width = PROBE_W; desc.height = PROBE_H;
    desc.num_objects = 1;
    desc.objects[0].fd = prime_fd;
    desc.objects[0].size = stride * PROBE_H;
    desc.objects[0].drm_format_modifier = DRM_FORMAT_MOD_LINEAR;
    desc.num_layers = 1;
    desc.layers[0].drm_format = DRM_FORMAT_ARGB8888;
    desc.layers[0].num_planes = 1;
    desc.layers[0].object_index[0] = 0;
    desc.layers[0].pitch[0] = stride;

    VASurfaceAttrib attrs[2];
    memset(attrs, 0, sizeof(attrs));
    attrs[0].type = VASurfaceAttribMemoryType; attrs[0].flags = VA_SURFACE_ATTRIB_SETTABLE;
    attrs[0].value.type = VAGenericValueTypeInteger;
    attrs[0].value.value.i = VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2;
    attrs[1].type = VASurfaceAttribExternalBufferDescriptor; attrs[1].flags = VA_SURFACE_ATTRIB_SETTABLE;
    attrs[1].value.type = VAGenericValueTypePointer;
    attrs[1].value.value.p = &desc;

    VASurfaceID argb = VA_INVALID_SURFACE;
    CHECK(vaCreateSurfaces(dpy, VA_RT_FORMAT_RGB32, PROBE_W, PROBE_H, &argb, 1, attrs, 2) == VA_STATUS_SUCCESS,
          "vaCreateSurfaces(import)");
    printf("IMPORT_OK surface=%u (0 CPU copies)\n", argb);

    // --- Stage B: VPP ARGB -> NV12, into a libav-owned pool surface -----
    // Allocate the NV12 surface from libav's own VAAPI frame pool so the
    // resulting AVFrame is native to the encoder (avoids a manual data[3]).
    AVFrame *frame = av_frame_alloc();
    CHECK(frame, "av_frame_alloc");
    CHECK(av_hwframe_get_buffer(frames_ref, frame, 0) >= 0, "av_hwframe_get_buffer");
    VASurfaceID nv12 = (VASurfaceID)(uintptr_t)frame->data[3];
    printf("libav pool NV12 surface=%u\n", nv12);

    VAConfigAttrib vpp_attr = { VAConfigAttribRTFormat, VA_RT_FORMAT_YUV420 };
    VAConfigID vpp_cfg = VA_INVALID_ID; VAContextID vpp_ctx = VA_INVALID_ID;
    CHECK(vaCreateConfig(dpy, VAProfileNone, VAEntrypointVideoProc, &vpp_attr, 1, &vpp_cfg) == VA_STATUS_SUCCESS,
          "vaCreateConfig(vpp)");
    CHECK(vaCreateContext(dpy, vpp_cfg, PROBE_W, PROBE_H, VA_PROGRESSIVE, &nv12, 1, &vpp_ctx) == VA_STATUS_SUCCESS,
          "vaCreateContext(vpp)");

    VAProcPipelineParameterBuffer pp; memset(&pp, 0, sizeof(pp));
    pp.surface = argb;
    pp.surface_color_standard = VAProcColorStandardNone;
    pp.output_background_color = 0xff000000u;
    VABufferID pp_buf = VA_INVALID_ID;
    CHECK(vaCreateBuffer(dpy, vpp_ctx, VAProcPipelineParameterBufferType, sizeof(pp), 1, &pp, &pp_buf) == VA_STATUS_SUCCESS,
          "vaCreateBuffer(vpp)");
    vaBeginPicture(dpy, vpp_ctx, nv12);
    vaRenderPicture(dpy, vpp_ctx, &pp_buf, 1);
    vaEndPicture(dpy, vpp_ctx);
    CHECK(vaSyncSurface(dpy, nv12) == VA_STATUS_SUCCESS, "vaSyncSurface(vpp)");
    printf("VPP_OK ARGB->NV12 surface=%u\n", nv12);

    // --- Stage C: hevc_vaapi encode of the NV12 surface (libavcodec) ---
    const AVCodec *codec = avcodec_find_encoder_by_name("hevc_vaapi");
    CHECK(codec, "hevc_vaapi encoder");
    AVCodecContext *avctx = avcodec_alloc_context3(codec);
    CHECK(avctx, "avcodec_alloc_context3");
    avctx->width = PROBE_W;
    avctx->height = PROBE_H;
    avctx->time_base = (AVRational){1, 60};
    avctx->framerate = (AVRational){60, 1};
    avctx->pix_fmt = AV_PIX_FMT_VAAPI;
    avctx->bit_rate = 8000000;
    avctx->gop_size = 30;
    avctx->max_b_frames = 0;
    avctx->hw_device_ctx = av_buffer_ref(device_ref);
    avctx->hw_frames_ctx = av_buffer_ref(frames_ref);
    av_opt_set(avctx->priv_data, "rc_mode", "CBR", 0);
    CHECK(avcodec_open2(avctx, codec, NULL) >= 0, "avcodec_open2");

    frame->pts = 0;

    AVPacket *pkt = av_packet_alloc();
    CHECK(pkt, "av_packet_alloc");
    FILE *out = fopen("/tmp/aqua_libav.hevc", "wb");
    CHECK(out, "fopen out");
    size_t total = 0;
    if (avcodec_send_frame(avctx, frame) >= 0) {
        while (avcodec_receive_packet(avctx, pkt) == 0) {
            fwrite(pkt->data, 1, pkt->size, out);
            total += pkt->size;
            av_packet_unref(pkt);
        }
    }
    avcodec_send_frame(avctx, NULL); // flush
    while (avcodec_receive_packet(avctx, pkt) == 0) {
        fwrite(pkt->data, 1, pkt->size, out);
        total += pkt->size;
        av_packet_unref(pkt);
    }
    fclose(out);
    printf("ENCODE_LIBAV_OK bytes=%zu -> /tmp/aqua_libav.hevc\n", total);

    av_frame_free(&frame);
    av_packet_free(&pkt);
    avcodec_free_context(&avctx);
    av_buffer_unref(&frames_ref);
    av_buffer_unref(&device_ref);
    gbm_bo_destroy(bo);
    gbm_device_destroy(gbm);
    close(drm_fd);
    return 0;
}
