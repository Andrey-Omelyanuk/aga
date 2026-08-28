# Веб-клиент: отдельный сервис, раздаёт SPA статикой (nginx).
# Ядро (main/) статику не раздаёт — dev.localhost ведёт сюда, API живёт
# на api.localhost в ядре.
FROM nginx:1.27-alpine

COPY index.html /usr/share/nginx/html/index.html

EXPOSE 80