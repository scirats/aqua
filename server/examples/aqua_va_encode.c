// Aqua phase 3C — encoder sidecar `aqua-va-encode`.
//
// The Aqua server creates/listens on a Unix SOCK_STREAM (path in
// AQUA_VA_SOCKET) and spawns this binary; we CONNECT to that path. One sidecar
// process per encoder session (per window).
//
// Per frame the server sends ONE sendmsg carrying:
//   REQUEST header (40 B, little-endian) + planes[16 B * num_planes] + the
//   dmabuf fds via SCM_RIGHTS (one fd per plane, in order).
// We answer with a 32 B RESPONSE header + payload (CONFIG / FRAME / ERROR).
//
// Pipeline (no CPU readback):
//   dmabuf fd -> VA surface (DRM_PRIME_2 import) -> VPP ARGB->NV12 (GPU)
//             -> hevc_vaapi / h264_vaapi (libavcodec, same VADisplay) -> Annex-B
//
// Build (Linux box, userland sysroot):
//   source ~/.aqua-p2p/sysroot-env.sh
//   S=$HOME/.local/sysroot
//   gcc -I$S/usr/include -I$S/usr/include/libdrm -I$S/usr/include/x86_64-linux-gnu \
//       server/examples/aqua_va_encode.c -o /tmp/aqua-va-encode \
//       -L$S/usr/lib/x86_64-linux-gnu -lgbm -ldrm -lavcodec -lavutil \
//       /usr/lib/x86_64-linux-gnu/libva.so.2 /usr/lib/x86_64-linux-gnu/libva-drm.so.2

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <unistd.h>
#include <errno.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/uio.h>

#include <va/va.h>
#include <va/va_drmcommon.h>
#include <va/va_vpp.h>
#include <drm_fourcc.h>

#include <libavcodec/avcodec.h>
#include <libavutil/hwcontext.h>
#include <libavutil/hwcontext_vaapi.h>
#include <libavutil/opt.h>

#define MAGIC 0x41564131u
#define MAX_PLANES 4
#define MAX_AU (4 * 1024 * 1024)

enum { K_FRAME = 1, K_FLUSH = 2, K_SHUTDOWN = 3 };
enum { R_CONFIG = 10, R_FRAME = 11, R_ERROR = 12, R_FLUSHED = 13 };

struct __attribute__((packed)) req_header {
    uint32_t magic;
    uint8_t  kind;
    uint8_t  codec;      // 1=H264, 2=HEVC
    uint16_t rsv;
    uint32_t fourcc;     // DRM fourcc of the dmabuf (e.g. AR24)
    uint32_t width;
    uint32_t height;
    uint64_t frame_id;
    uint64_t pts_us;
    uint8_t  keyframe;
    uint8_t  num_planes;
    uint16_t rsv2;
};

struct __attribute__((packed)) req_plane {
    uint32_t offset;
    uint32_t stride;
    uint64_t modifier;
};

struct __attribute__((packed)) resp_header {
    uint32_t magic;
    uint8_t  kind;
    uint8_t  codec;
    uint16_t rsv;
    uint32_t width;
    uint32_t height;
    uint64_t frame_id;
    uint8_t  keyframe;
    uint8_t  rsv3[3];
    uint32_t payload_len;
};

typedef struct {
    int inited;
    int codec;          // 1 H264 / 2 HEVC
    uint32_t fourcc;
    uint32_t width, height;

    AVBufferRef *device_ref;
    AVBufferRef *frames_ref;
    AVCodecContext *avctx;

    VAConfigID vpp_cfg;
    VAContextID vpp_ctx;

    uint8_t config[8192];
    int config_len;
    int config_sent;
} Encoder;

static VADisplay g_dpy = NULL;
static AVBufferRef *g_device = NULL;

static uint32_t va_fourcc_from_drm(uint32_t drm) {
    switch (drm) {
        case DRM_FORMAT_ARGB8888: return VA_FOURCC_ARGB;
        case DRM_FORMAT_XRGB8888: return VA_FOURCC_XRGB;
        case DRM_FORMAT_ABGR8888: return VA_FOURCC_ABGR;
        case DRM_FORMAT_XBGR8888: return VA_FOURCC_XBGR;
        case DRM_FORMAT_NV12:     return VA_FOURCC_NV12;
        case DRM_FORMAT_P010:     return VA_FOURCC_P010;
        default:                  return VA_FOURCC_ARGB;
    }
}

static int ensure_device(void) {
    if (g_device) return 0;
    if (av_hwdevice_ctx_create(&g_device, AV_HWDEVICE_TYPE_VAAPI,
                               "/dev/dri/renderD128", NULL, 0) < 0) {
        return -1;
    }
    AVHWDeviceContext *dev = (AVHWDeviceContext *)g_device->data;
    AVVAAPIDeviceContext *vac = (AVVAAPIDeviceContext *)dev->hwctx;
    g_dpy = vac->display;
    return 0;
}

static void encoder_teardown(Encoder *e) {
    if (e->vpp_ctx != VA_INVALID_ID) vaDestroyContext(g_dpy, e->vpp_ctx);
    if (e->vpp_cfg != VA_INVALID_ID) vaDestroyConfig(g_dpy, e->vpp_cfg);
    if (e->avctx) avcodec_free_context(&e->avctx);
    if (e->frames_ref) av_buffer_unref(&e->frames_ref);
    e->vpp_ctx = VA_INVALID_ID;
    e->vpp_cfg = VA_INVALID_ID;
    e->inited = 0;
    e->config_sent = 0;
    e->config_len = 0;
}

static int encoder_setup(Encoder *e, int codec, uint32_t fourcc, uint32_t w, uint32_t h) {
    encoder_teardown(e);
    if (ensure_device() < 0) return -1;

    if (codec != e->codec || w != e->width || h != e->height) e->config_sent = 0;
    e->codec = codec;
    e->fourcc = fourcc;
    e->width = w;
    e->height = h;

    // NV12 frame pool on libav's device.
    e->frames_ref = av_hwframe_ctx_alloc(g_device);
    if (!e->frames_ref) return -1;
    AVHWFramesContext *fc = (AVHWFramesContext *)e->frames_ref->data;
    fc->format = AV_PIX_FMT_VAAPI;
    fc->sw_format = AV_PIX_FMT_NV12;
    fc->width = w;
    fc->height = h;
    fc->initial_pool_size = 4;
    if (av_hwframe_ctx_init(e->frames_ref) < 0) return -1;

    const char *name = (codec == 1) ? "h264_vaapi" : "hevc_vaapi";
    const AVCodec *c = avcodec_find_encoder_by_name(name);
    if (!c) { fprintf(stderr, "encoder %s not found\n", name); return -1; }
    e->avctx = avcodec_alloc_context3(c);
    e->avctx->width = w;
    e->avctx->height = h;
    e->avctx->time_base = (AVRational){1, 60};
    e->avctx->framerate = (AVRational){60, 1};
    e->avctx->pix_fmt = AV_PIX_FMT_VAAPI;
    e->avctx->bit_rate = 8000000;
    e->avctx->gop_size = 120;
    e->avctx->max_b_frames = 0;
    e->avctx->hw_device_ctx = av_buffer_ref(g_device);
    e->avctx->hw_frames_ctx = av_buffer_ref(e->frames_ref);
    av_opt_set(e->avctx->priv_data, "rc_mode", "CBR", 0);
    av_opt_set(e->avctx->priv_data, "async_depth", "1", 0);
    e->avctx->flags |= AV_CODEC_FLAG_LOW_DELAY;
    if (avcodec_open2(e->avctx, c, NULL) < 0) {
        fprintf(stderr, "avcodec_open2(%s) failed\n", name);
        return -1;
    }

    VAConfigAttrib vpp_attr = { VAConfigAttribRTFormat, VA_RT_FORMAT_YUV420 };
    if (vaCreateConfig(g_dpy, VAProfileNone, VAEntrypointVideoProc, &vpp_attr, 1, &e->vpp_cfg) != VA_STATUS_SUCCESS)
        return -1;

    e->inited = 1;
    return 0;
}

static int import_dmabuf(Encoder *e, const struct req_header *h,
                         const struct req_plane *planes, const int *fds,
                         VASurfaceID *out) {
    VADRMPRIMESurfaceDescriptor desc;
    memset(&desc, 0, sizeof(desc));
    desc.fourcc = va_fourcc_from_drm(h->fourcc);
    desc.width = h->width;
    desc.height = h->height;
    desc.num_objects = 1;
    desc.objects[0].fd = fds[0];
    desc.objects[0].size = planes[0].stride * h->height;
    desc.objects[0].drm_format_modifier = planes[0].modifier;
    desc.num_layers = 1;
    desc.layers[0].drm_format = h->fourcc;
    desc.layers[0].num_planes = 1;
    desc.layers[0].object_index[0] = 0;
    desc.layers[0].offset[0] = planes[0].offset;
    desc.layers[0].pitch[0] = planes[0].stride;

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

    unsigned int rt = (h->fourcc == DRM_FORMAT_NV12 || h->fourcc == DRM_FORMAT_P010)
                          ? VA_RT_FORMAT_YUV420 : VA_RT_FORMAT_RGB32;
    return vaCreateSurfaces(g_dpy, rt, h->width, h->height, out, 1, attrs, 2);
}

// Split an Annex-B access unit into (config = VPS/SPS/PPS) and (picture = rest).
static void split_au(const uint8_t *buf, int len, int codec,
                     uint8_t *config, int *config_len,
                     uint8_t *picture, int *picture_len) {
    *config_len = 0;
    *picture_len = 0;
    int i = 0;
    while (i + 4 <= len) {
        // find start code 00 00 01 / 00 00 00 01
        if (buf[i] == 0 && buf[i+1] == 0 && buf[i+2] == 1) {
            int nal = i + 3;
            int j = nal;
            while (j + 4 <= len) {
                if (buf[j] == 0 && buf[j+1] == 0 && (buf[j+2] == 1 || (buf[j+2] == 0 && buf[j+3] == 1))) break;
                j++;
            }
            int end = (j + 4 > len) ? len : j;
            if (nal < end) {
                int type;
                if (codec == 1) type = buf[nal] & 0x1f;
                else type = (buf[nal] >> 1) & 0x3f;
                int is_param = (codec == 1) ? (type == 7 || type == 8)
                                            : (type == 32 || type == 33 || type == 34);
                uint8_t *dst = is_param ? config : picture;
                int *dst_len = is_param ? config_len : picture_len;
                if (*dst_len + (end - i) <= 8192) {
                    memcpy(dst + *dst_len, buf + i, end - i);
                    *dst_len += end - i;
                }
            }
            i = end;
        } else {
            i++;
        }
    }
}

static int send_response(int fd, uint8_t kind, uint8_t codec, uint32_t w, uint32_t h,
                         uint64_t frame_id, uint8_t keyframe,
                         const uint8_t *payload, uint32_t payload_len) {
    struct resp_header rh;
    memset(&rh, 0, sizeof(rh));
    rh.magic = MAGIC;
    rh.kind = kind;
    rh.codec = codec;
    rh.width = w;
    rh.height = h;
    rh.frame_id = frame_id;
    rh.keyframe = keyframe;
    rh.payload_len = payload_len;

    struct iovec iov[2];
    iov[0].iov_base = &rh; iov[0].iov_len = sizeof(rh);
    iov[1].iov_base = (void *)payload; iov[1].iov_len = payload_len;
    struct msghdr msg;
    memset(&msg, 0, sizeof(msg));
    msg.msg_iov = iov;
    msg.msg_iovlen = payload_len ? 2 : 1;
    return sendmsg(fd, &msg, 0) < 0 ? -1 : 0;
}

static int handle_frame(Encoder *e, int sock, const struct req_header *h,
                        const struct req_plane *planes, const int *fds) {
    if (!e->inited || e->width != h->width || e->height != h->height ||
        e->codec != h->codec || e->fourcc != h->fourcc) {
        if (encoder_setup(e, h->codec, h->fourcc, h->width, h->height) < 0) {
            send_response(sock, R_ERROR, h->codec, h->width, h->height, h->frame_id, 0,
                          (const uint8_t *)"encoder setup failed", 21);
            return -1;
        }
    }

    VASurfaceID src = VA_INVALID_SURFACE;
    VAStatus ist = import_dmabuf(e, h, planes, fds, &src);
    if (ist != VA_STATUS_SUCCESS) {
        send_response(sock, R_ERROR, h->codec, h->width, h->height, h->frame_id, 0,
                      (const uint8_t *)"dmabuf import failed", 20);
        return 0;
    }

    AVFrame *frame = av_frame_alloc();
    int hb = av_hwframe_get_buffer(e->frames_ref, frame, 0);
    if (hb < 0) {
        av_frame_free(&frame);
        vaDestroySurfaces(g_dpy, &src, 1);
        return 0;
    }
    VASurfaceID dst = (VASurfaceID)(uintptr_t)frame->data[3];

    // VPP ARGB -> NV12 into the encoder pool surface. The VPP context is bound
    // to the destination surface (Mesa requires the render target at creation).
    VAContextID vctx = VA_INVALID_ID;
    VAStatus cst = vaCreateContext(g_dpy, e->vpp_cfg, h->width, h->height, VA_PROGRESSIVE,
                                   &dst, 1, &vctx);
    if (cst == VA_STATUS_SUCCESS) {
        VAProcPipelineParameterBuffer pp;
        memset(&pp, 0, sizeof(pp));
        pp.surface = src;
        pp.surface_color_standard = VAProcColorStandardNone;
        pp.output_background_color = 0xff000000u;
        VABufferID pp_buf = VA_INVALID_ID;
        VAStatus bst = vaCreateBuffer(g_dpy, vctx, VAProcPipelineParameterBufferType,
                                      sizeof(pp), 1, &pp, &pp_buf);
        if (bst == VA_STATUS_SUCCESS) {
            vaBeginPicture(g_dpy, vctx, dst);
            vaRenderPicture(g_dpy, vctx, &pp_buf, 1);
            vaEndPicture(g_dpy, vctx);
            VAStatus sst = vaSyncSurface(g_dpy, dst);
            vaDestroyBuffer(g_dpy, pp_buf);
        }
        vaDestroyContext(g_dpy, vctx);
    }
    vaDestroySurfaces(g_dpy, &src, 1);

    frame->pts = (int64_t)h->pts_us;
    frame->duration = 1;
    if (h->keyframe) {
        frame->pict_type = AV_PICTURE_TYPE_I;
    }

    int ret = avcodec_send_frame(e->avctx, frame);
    if (ret < 0) { av_frame_free(&frame); return 0; }

    AVPacket *pkt = av_packet_alloc();
    static uint8_t cfg[8192];
    static uint8_t pic[MAX_AU];
    int rret = 0;
    while ((rret = avcodec_receive_packet(e->avctx, pkt)) == 0) {
        int cfg_len = 0, pic_len = 0;
        split_au(pkt->data, pkt->size, h->codec, cfg, &cfg_len, pic, &pic_len);

        if (cfg_len > 0 && (!e->config_sent || cfg_len != e->config_len ||
                            memcmp(cfg, e->config, cfg_len) != 0)) {
            memcpy(e->config, cfg, cfg_len);
            e->config_len = cfg_len;
            send_response(sock, R_CONFIG, h->codec, h->width, h->height, h->frame_id, 0,
                          e->config, e->config_len);
            e->config_sent = 1;
        }
        if (pic_len > 0) {
            int key = (pkt->flags & AV_PKT_FLAG_KEY) ? 1 : 0;
            send_response(sock, R_FRAME, h->codec, h->width, h->height, h->frame_id,
                          (uint8_t)key, pic, pic_len);
        }
        av_packet_unref(pkt);
    }
    av_packet_free(&pkt);
    av_frame_free(&frame);
    return 0;
}

int main(void) {
    const char *path = getenv("AQUA_VA_SOCKET");
    if (!path) { fprintf(stderr, "AQUA_VA_SOCKET not set\n"); return 1; }

    int sock = socket(AF_UNIX, SOCK_STREAM, 0);
    if (sock < 0) { perror("socket"); return 1; }
    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    strncpy(addr.sun_path, path, sizeof(addr.sun_path) - 1);
    if (connect(sock, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        perror("connect"); return 1;
    }
    fprintf(stderr, "aqua-va-encode: connected to %s\n", path);

    Encoder enc;
    memset(&enc, 0, sizeof(enc));
    enc.vpp_cfg = VA_INVALID_ID;
    enc.vpp_ctx = VA_INVALID_ID;

    for (;;) {
        struct req_header h;
        struct req_plane planes[MAX_PLANES];
        int fds[MAX_PLANES];

        uint8_t buf[sizeof(struct req_header) + sizeof(struct req_plane) * MAX_PLANES];
        uint8_t cmsg_buf[CMSG_SPACE(sizeof(int) * MAX_PLANES)];
        struct iovec iov = { buf, sizeof(struct req_header) };
        struct msghdr msg;
        memset(&msg, 0, sizeof(msg));
        msg.msg_iov = &iov;
        msg.msg_iovlen = 1;
        msg.msg_control = cmsg_buf;
        msg.msg_controllen = sizeof(cmsg_buf);

        ssize_t n = recvmsg(sock, &msg, MSG_WAITALL);
        if (n <= 0) break;
        if (n < (ssize_t)sizeof(struct req_header)) break;
        memcpy(&h, buf, sizeof(h));
        if (h.magic != MAGIC) break;

        int num = h.num_planes;
        if (num < 1) num = 1;
        if (num > MAX_PLANES) num = MAX_PLANES;

        // read the plane descriptors (may already be in buf if short header)
        int got = (int)n - (int)sizeof(h);
        int need = num * (int)sizeof(struct req_plane);
        if (got < need) {
            if (recv(sock, buf + sizeof(h) + got, need - got, MSG_WAITALL) < 0) break;
        }
        memcpy(planes, buf + sizeof(h), need);

        int nfd = 0;
        for (struct cmsghdr *c = CMSG_FIRSTHDR(&msg); c; c = CMSG_NXTHDR(&msg, c)) {
            if (c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_RIGHTS) {
                int cnt = (c->cmsg_len - CMSG_LEN(0)) / sizeof(int);
                for (int i = 0; i < cnt && nfd < MAX_PLANES; i++)
                    memcpy(&fds[nfd++], CMSG_DATA(c) + i * sizeof(int), sizeof(int));
            }
        }

        if (h.kind == K_SHUTDOWN) {
            for (int i = 0; i < nfd; i++) close(fds[i]);
            break;
        }
        if (h.kind == K_FLUSH) {
            send_response(sock, R_FLUSHED, h.codec, h.width, h.height, h.frame_id, 0, NULL, 0);
            for (int i = 0; i < nfd; i++) close(fds[i]);
            continue;
        }
        if (h.kind == K_FRAME && nfd >= 1) {
            handle_frame(&enc, sock, &h, planes, fds);
            for (int i = 0; i < nfd; i++) close(fds[i]);
        }
    }

    encoder_teardown(&enc);
    if (g_device) av_buffer_unref(&g_device);
    close(sock);
    return 0;
}
