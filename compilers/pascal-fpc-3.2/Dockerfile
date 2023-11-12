FROM alpine:3.17.0
ARG USER_ID=1000
ARG GROUP_ID=1000
RUN addgroup -g $GROUP_ID -S judge && \
    adduser -u $USER_ID -D -S -G judge -s /bin/sh judge && \
    apk add --no-cache \
    binutils=2.39-r2 && \
    apk add --no-cache --repository=http://dl-cdn.alpinelinux.org/alpine/edge/testing \
    fpc=3.2.2-r2
USER judge:judge
WORKDIR /home/judge
