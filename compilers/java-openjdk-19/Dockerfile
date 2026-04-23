FROM rust:1.77-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY compile/ /src/
RUN cargo build --release

FROM alpine:3.19.0
ARG USER_ID=1000
ARG GROUP_ID=1000
COPY --from=build /src/target/release/judge-java-compile /bin/judge-java-compile
RUN addgroup -g $GROUP_ID -S judge && \
    adduser -u $USER_ID -D -S -G judge -s /bin/sh judge && \
    chmod +x /bin/judge-java-compile && \
    apk add --no-cache --repository=http://dl-cdn.alpinelinux.org/alpine/edge/community \
    openjdk19=19.0.2_p7-r0 && \
    # Remove unnecessary files to reduce image size.
    rm -rf /usr/lib/jvm/java-19-openjdk/lib/src.zip \
           /usr/lib/jvm/java-19-openjdk/include \
           /usr/lib/jvm/java-19-openjdk/man \
           /usr/lib/jvm/java-19-openjdk/legal \
           /usr/share/doc \
           /usr/share/man \
           /var/cache/apk/*
USER judge:judge
WORKDIR /home/judge
