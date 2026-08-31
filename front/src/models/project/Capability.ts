import { Model, model, field, id, NUMBER, STRING } from 'mobx-model-ui';
import { api } from '@/services/http-adapter';
import type { CatalogVersion } from './AgentSet';

/** Каталог способностей: скилл или команда с версиями. */
abstract class Capability extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) name!: string;
  @field() versions: CatalogVersion[] = [];

  /** Версии каталога как строки для выпадающего списка. */
  get versionNames(): string[] {
    return this.versions.map((v) => v.version);
  }
}

@api('skills')
@model
export class Skill extends Capability {}

@api('commands')
@model
export class Command extends Capability {}