FROM rust:1.77-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY compile/ /src/
RUN cargo build --release

FROM alpine:3.17.0
ARG USER_ID=1000
ARG GROUP_ID=1000
COPY --from=build /src/target/release/judge-gcc-compile /bin/judge-gcc-compile
RUN addgroup -g $GROUP_ID -S judge && \
    adduser -u $USER_ID -D -S -G judge -s /bin/sh judge && \
    chmod +x /bin/judge-gcc-compile && \
    apk add --no-cache \
    gcc=12.2.1_git20220924-r4 \
    musl-dev=1.2.3-r6 && \
    rm -rf /usr/share/doc \
           /usr/share/man \
           /var/cache/apk/*
USER judge:judge
WORKDIR /home/judge
