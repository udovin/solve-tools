FROM alpine:3.17.0
ARG USER_ID=1000
ARG GROUP_ID=1000
RUN addgroup -g $GROUP_ID -S judge && \
    adduser -u $USER_ID -D -S -G judge -s /bin/sh judge && \
    apk add --no-cache \
    gcc=12.2.1_git20220924-r4 \
    musl-dev=1.2.3-r5
USER judge:judge
WORKDIR /home/judge
