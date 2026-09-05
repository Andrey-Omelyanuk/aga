FROM docker:27-dind

RUN apk add --no-cache git ca-certificates

# Пользователь воркстейшна (uid 1000) с домашним каталогом. В dev-режиме ядро
# выполняет команды агента через `docker exec -u 1000:1000` — ему нужен свой
# home для SSH-конфига агента (~/.ssh), куда ядро кладёт приватный ключ
# (inject_ssh_key). Без записи в /etc/passwd docker не знал бы HOME этого uid.
RUN adduser -D -u 1000 -h /home/aga aga

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

# docker:dind по умолчанию стартует dockerd; здесь воркстейшн сам управляет
# своим демоном и подготавливает проект.
ENTRYPOINT []
CMD ["/entrypoint.sh"]