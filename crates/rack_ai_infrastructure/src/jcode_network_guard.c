#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <netinet/in.h>
#include <string.h>
#include <sys/socket.h>

static int allowed(const struct sockaddr *addr) {
    if (!addr) return 1;

    if (addr->sa_family == AF_INET) {
        const struct sockaddr_in *a = (const struct sockaddr_in *)addr;
        unsigned char *p = (unsigned char *)&a->sin_addr.s_addr;
        return p[0] == 127;
    }

    if (addr->sa_family == AF_INET6) {
        const struct sockaddr_in6 *a = (const struct sockaddr_in6 *)addr;
        return IN6_IS_ADDR_LOOPBACK(&a->sin6_addr);
    }

    return 1;
}

int connect(int fd, const struct sockaddr *addr, socklen_t len) {
    static int (*real_connect)(int, const struct sockaddr *, socklen_t);
    if (!real_connect) {
        real_connect = dlsym(RTLD_NEXT, "connect");
    }

    if (!allowed(addr)) {
        errno = ENETUNREACH;
        return -1;
    }

    return real_connect(fd, addr, len);
}

ssize_t sendto(
    int fd,
    const void *buf,
    size_t len,
    int flags,
    const struct sockaddr *addr,
    socklen_t addrlen
) {
    static ssize_t (*real_sendto)(
        int,
        const void *,
        size_t,
        int,
        const struct sockaddr *,
        socklen_t
    );
    if (!real_sendto) {
        real_sendto = dlsym(RTLD_NEXT, "sendto");
    }

    if (addr && !allowed(addr)) {
        errno = ENETUNREACH;
        return -1;
    }

    return real_sendto(fd, buf, len, flags, addr, addrlen);
}
