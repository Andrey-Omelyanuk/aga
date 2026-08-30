import { Adapter, ID, Query, type Model } from 'mobx-model-ui';
import { HttpClient } from './http';

export class RESTAdapter<M extends Model> extends Adapter<M> {
  constructor(
    private readonly base: string,
    private readonly http: HttpClient,
  ) {
    super();
  }

  private url(id?: ID, name?: string): string {
    let path = this.base;
    if (id !== undefined) path += `/${id}`;
    if (name !== undefined) path += `/${name}`;
    return path;
  }

  create(rawData: any): Promise<any> {
    return this.http.send(this.url(), 'POST', rawData).then((r) => r.json());
  }

  update(id: ID, onlyChanged: any): Promise<any> {
    return this.http
      .send(this.url(id), 'PATCH', onlyChanged)
      .then((r) => r.json());
  }

  async delete(id: ID): Promise<void> {
    await this.http.send(this.url(id), 'DELETE');
  }

  action(id: ID, name: string, kwargs: Record<string, any>): Promise<any> {
    return this.http.send(this.url(id, name), 'POST', kwargs).then((r) => r.json());
  }

  modelAction(name: string, kwargs: Record<string, any>): Promise<any> {
    return this.http.send(this.url(undefined, name), 'POST', kwargs).then((r) => r.json());
  }

  get(id: ID): Promise<any> {
    return this.http.json(this.url(id));
  }

  find(_query: Query<M>): Promise<any> {
    return Promise.resolve(null);
  }

  load(_query: Query<M>): Promise<any[]> {
    return this.http.json(this.url());
  }

  getTotalCount(): Promise<number> {
    return Promise.resolve(0);
  }

  getDistinct(): Promise<any[]> {
    return Promise.resolve([]);
  }

  getURLSearchParams(_query: Query<M>): URLSearchParams {
    return new URLSearchParams();
  }
}