import { useEffect } from 'react';
import { config } from 'mobx-model-ui';
import { useLocation, useSearchParams } from 'react-router-dom';

// Синхронизация mobx-model-ui (Variable/Input с syncURL) с URL-строкой.
// Вызывается один раз в корневом layout — до создания любых syncURL-входов.

const url_changed_callbacks = new Set<(params: URLSearchParams) => void>();

const useMobX_ORM = () => {
  const [searchParams, setSearchParams] = useSearchParams();
  const location = useLocation();

  config.UPDATE_SEARCH_PARAMS = (search_params: URLSearchParams) => {
    if (search_params.toString() === searchParams.toString()) return;
    setSearchParams(search_params);
  };
  config.WATCH_URL_CHANGES = (callback: (params: URLSearchParams) => void) => {
    url_changed_callbacks.add(callback);
    return () => {
      url_changed_callbacks.delete(callback);
    };
  };

  useEffect(() => {
    url_changed_callbacks.forEach((callback) => callback(searchParams));
  }, [location]);
};

export default useMobX_ORM;