// Sidecar self-test: acts as the Aqua server side, sends one dmabuf FRAME to
// aqua-va-encode and prints the CONFIG/FRAME responses.
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <signal.h>

#include <gbm.h>
#include <drm_fourcc.h>

#define MAGIC 0x41564131u

struct __attribute__((packed)) req_header {
    uint32_t magic; uint8_t kind; uint8_t codec; uint16_t rsv;
    uint32_t fourcc; uint32_t width; uint32_t height;
    uint64_t frame_id; uint64_t pts_us; uint8_t keyframe; uint8_t num_planes; uint16_t rsv2;
};
struct __attribute__((packed)) req_plane { uint32_t offset; uint32_t stride; uint64_t modifier; };
struct __attribute__((packed)) resp_header {
    uint32_t magic; uint8_t kind; uint8_t codec; uint16_t rsv;
    uint32_t width; uint32_t height; uint64_t frame_id;
    uint8_t keyframe; uint8_t rsv3[3]; uint32_t payload_len;
};

int main(void) {
    const char *path = "/tmp/aqua-va-test.sock";
    const int W = 256, H = 256;

    // GBM LINEAR ARGB dmabuf
    int drm = open("/dev/dri/renderD128", O_RDWR | O_CLOEXEC);
    struct gbm_device *gbm = gbm_create_device(drm);
    struct gbm_bo *bo = gbm_bo_create(gbm, W, H, GBM_FORMAT_ARGB8888, GBM_BO_USE_LINEAR);
    int fd = gbm_bo_get_fd(bo);
    uint32_t stride = gbm_bo_get_stride(bo);

    unlink(path);
    int ls = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un addr; memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX; strncpy(addr.sun_path, path, sizeof(addr.sun_path) - 1);
    if (bind(ls, (struct sockaddr *)&addr, sizeof(addr)) < 0) { perror("bind"); return 1; }
    listen(ls, 1);

    pid_t pid = fork();
    if (pid == 0) {
        setenv("AQUA_VA_SOCKET", path, 1);
        execl("/tmp/aqua-va-encode", "aqua-va-encode", (char *)NULL);
        _exit(127);
    }
    int cs = accept(ls, NULL, NULL);
    if (cs < 0) { perror("accept"); return 1; }
    usleep(200000);

    // send one FRAME
    struct req_header h; memset(&h, 0, sizeof(h));
    h.magic = MAGIC; h.kind = 1; h.codec = 2; h.fourcc = DRM_FORMAT_ARGB8888;
    h.width = W; h.height = H; h.frame_id = 1; h.pts_us = 0; h.keyframe = 1; h.num_planes = 1;
    struct req_plane pl = { 0, stride, 0 };

    struct iovec iov[2];
    iov[0].iov_base = &h; iov[0].iov_len = sizeof(h);
    iov[1].iov_base = &pl; iov[1].iov_len = sizeof(pl);
    uint8_t cbuf[CMSG_SPACE(sizeof(int))];
    struct msghdr msg; memset(&msg, 0, sizeof(msg));
    msg.msg_iov = iov; msg.msg_iovlen = 2;
    msg.msg_control = cbuf; msg.msg_controllen = sizeof(cbuf);
    struct cmsghdr *c = CMSG_FIRSTHDR(&msg);
    c->cmsg_level = SOL_SOCKET; c->cmsg_type = SCM_RIGHTS; c->cmsg_len = CMSG_LEN(sizeof(int));
    memcpy(CMSG_DATA(c), &fd, sizeof(int));
    if (sendmsg(cs, &msg, 0) < 0) { perror("sendmsg"); return 1; }
    printf("sent FRAME id=1 fourcc=AR24 %dx%d stride=%u fd=%d\n", W, H, stride, fd);

    // read responses (CONFIG then FRAME) with a short timeout
    struct timeval tv = {3, 0};
    setsockopt(cs, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    for (int n = 0; n < 4; n++) {
        struct resp_header rh;
        ssize_t r = recv(cs, &rh, sizeof(rh), MSG_WAITALL);
        if (r <= 0) { printf("no more responses (r=%zd)\n", r); break; }
        if (rh.magic != MAGIC) { printf("BAD magic 0x%x\n", rh.magic); break; }
        uint8_t payload[1 << 20];
        if (rh.payload_len && recv(cs, payload, rh.payload_len, MSG_WAITALL) <= 0) break;
        printf("RESPONSE kind=%u codec=%u %ux%u frame_id=%llu keyframe=%u bytes=%u\n",
               rh.kind, rh.codec, rh.width, rh.height,
               (unsigned long long)rh.frame_id, rh.keyframe, rh.payload_len);
        if (rh.kind == 11) break; // got a FRAME
    }

    // shutdown
    struct req_header sd; memset(&sd, 0, sizeof(sd));
    sd.magic = MAGIC; sd.kind = 3;
    send(cs, &sd, sizeof(sd), 0);
    close(cs);
    usleep(100000);
    kill(pid, SIGKILL);
    waitpid(pid, NULL, 0);
    close(ls);
    unlink(path);
    gbm_bo_destroy(bo); gbm_device_destroy(gbm); close(drm);
    return 0;
}
