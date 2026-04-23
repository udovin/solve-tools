FROM rust:1.77-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY compile/ /src/
RUN cargo build --release

FROM alpine:3.17.0
ARG USER_ID=1000
ARG GROUP_ID=1000
COPY --from=build /src/target/release/judge-go-compile /bin/judge-go-compile
RUN addgroup -g $GROUP_ID -S judge && \
    adduser -u $USER_ID -D -S -G judge -s /bin/sh judge && \
    chmod +x /bin/judge-go-compile && \
    apk add --no-cache \
    go=1.19.9-r0
USER judge:judge
WORKDIR /home/judge
