FROM docker:27-dind

RUN apk add --no-cache git ca-certificates

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

# docker:dind по умолчанию стартует dockerd; здесь воркстейшн сам управляет
# своим демоном и подготавливает проект.
ENTRYPOINT []
CMD ["/entrypoint.sh"]