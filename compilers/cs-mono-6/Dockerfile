FROM rust:1.77-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY compile/ /src/
RUN cargo build --release

FROM alpine:3.17.0
ARG USER_ID=1000
ARG GROUP_ID=1000
COPY --from=build /src/target/release/judge-mcs-compile /bin/judge-mcs-compile
RUN addgroup -g $GROUP_ID -S judge && \
    adduser -u $USER_ID -D -S -G judge -s /bin/sh judge && \
    chmod +x /bin/judge-mcs-compile && \
    apk add --no-cache --repository=http://dl-cdn.alpinelinux.org/alpine/edge/community \
    mono=6.14.1-r1
USER judge:judge
WORKDIR /home/judge
