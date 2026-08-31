# Веб-клиент: собирается как React SPA, раздаётся nginx статикой из dist/.
# Ядро (main/) статику не раздаёт — dev.localhost ведёт сюда, API живёт
# на api.localhost в ядре. API_ENDPOINT подставляется в index.html при
# старте контейнера (replace-env.sh) — образ не зашивает адрес ядра.
FROM node:22-alpine AS build
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
RUN npm run build

FROM nginx:1.27-alpine
COPY --from=build /app/dist /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY replace-env.sh /docker-entrypoint.d/
RUN chmod +x /docker-entrypoint.d/replace-env.sh
ENV API_ENDPOINT=''
EXPOSE 80
CMD ["/docker-entrypoint.d/replace-env.sh", "nginx", "-g", "daemon off;"]