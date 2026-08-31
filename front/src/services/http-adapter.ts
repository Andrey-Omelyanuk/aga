import { Adapter, Filter, Model, Query, RequestConfig, ID } from 'mobx-model-ui';

import http from './http';

export class HttpAdapter<M extends Model> extends Adapter<M> {
  readonly endpoint: string;

  constructor(endpoint?: string) {
    super();
    // Без trailing slash: ядро (Axum) матчит маршруты точно (/projects,
    // /projects/:id, /chats/:id/messages), в отличие от Django-эталона.
    this.endpoint = endpoint ?? '';
  }

  private url(id?: ID, name?: string): string {
    let path = this.endpoint;
    if (id !== undefined) path += `/${id}`;
    if (name !== undefined) path += `/${name}`;
    return path;
  }

  async create(rawData: any, config?: RequestConfig): Promise<any> {
    const response = await http.post(this.url(), rawData, {
      onUploadProgress: config?.onUploadProgress as any,
    });
    return response.data;
  }

  async update(id: ID, onlyChangedRawData: any, config?: RequestConfig): Promise<any> {
    const response = await http.patch(this.url(id), onlyChangedRawData, {
      onUploadProgress: config?.onUploadProgress as any,
    });
    return response.data;
  }

  async delete(id: ID, _config?: RequestConfig): Promise<void> {
    await http.delete(this.url(id));
  }

  async action(id: ID, name: string, kwargs: Record<string, any>, config?: RequestConfig): Promise<any> {
    const response = await http.post(this.url(id, name), kwargs, {
      onUploadProgress: config?.onUploadProgress as any,
    });
    return response.data;
  }

  async modelAction(name: string, kwargs: Record<string, any>, config?: RequestConfig): Promise<any> {
    const response = await http.post(this.url(undefined, name), kwargs, {
      onUploadProgress: config?.onUploadProgress as any,
    });
    return response.data;
  }

  async get(id: ID, config?: RequestConfig): Promise<any> {
    const response = await http.get(this.url(id), {
      signal: config?.controller?.signal,
    });
    return response.data;
  }

  async find(query: Query<M>, config?: RequestConfig): Promise<any> {
    const queryParams = this.getURLSearchParams(query);
    const response = await http.get(`${this.url()}?${queryParams.toString()}`, {
      signal: config?.controller?.signal,
    });
    return response.data;
  }

  async load(query: Query<M>, config?: RequestConfig): Promise<any[]> {
    const queryParams = this.getURLSearchParams(query);
    const response = await http.get(`${this.url()}?${queryParams.toString()}`, {
      signal: config?.controller?.signal,
    });
    return response.data;
  }

  async getTotalCount(filter: Filter | undefined, config?: RequestConfig): Promise<number> {
    const searchParams = filter ? filter.URLSearchParams : new URLSearchParams();
    const response = await http.get(`${this.url()}count/?${searchParams.toString()}`, {
      signal: config?.controller?.signal,
    });
    return response.data;
  }

  async getDistinct(filter: Filter, field: string, config?: RequestConfig): Promise<any[]> {
    const searchParams = filter ? filter.URLSearchParams : new URLSearchParams();
    searchParams.set('__distinct', field);
    const response = await http.get(`${this.url()}distinct/?${searchParams.toString()}`, {
      signal: config?.controller?.signal,
    });
    return response.data;
  }

  getURLSearchParams(query: Query<M>): URLSearchParams {
    const searchParams = query.filter ? query.filter.URLSearchParams : new URLSearchParams();
    if (query.orderBy.value.length) searchParams.set('__order_by', query.orderBy.toString());
    if (query.limit.value !== undefined) searchParams.set('__limit', query.limit.toString());
    if (query.offset.value !== undefined) searchParams.set('__offset', query.offset.toString());
    if (query.relations.value.length) searchParams.set('__relations', query.relations.toString());
    if (query.fields.value.length) searchParams.set('__fields', query.fields.toString());
    if (query.omit.value.length) searchParams.set('__omit', query.omit.toString());
    return searchParams;
  }
}

// Декоратор модели: назначает REST-репозиторий по endpoint.
export function api(endpoint: string) {
  return (cls: any) => {
    cls.defaultRepository.adapter = new HttpAdapter(endpoint);
  };
}