FROM golang:1.20-alpine AS build
RUN apk add --no-cache git gcc linux-headers libc-dev make
WORKDIR /src/compile
COPY compile.go /src/compile
RUN go build compile.go

FROM alpine:3.19.0
ARG USER_ID=1000
ARG GROUP_ID=1000
COPY --from=build /src/compile/compile /bin/judge-java-compile
RUN addgroup -g $GROUP_ID -S judge && \
    adduser -u $USER_ID -D -S -G judge -s /bin/sh judge && \
    chmod +x /bin/judge-java-compile && \
    apk add --no-cache --repository=http://dl-cdn.alpinelinux.org/alpine/edge/community \
    openjdk19=19.0.2_p7-r0
USER judge:judge
WORKDIR /home/judge
