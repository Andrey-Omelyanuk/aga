/** Троттлинг с первым и последним вызовом: первый вызов выполняется сразу,
 *  вызовы внутри окна `ms` схлопываются в один повтор в конце окна. Для
 *  перезагрузок по событиям реального времени: всплеск сообщений (агент пишет
 *  шаги) даёт два запроса, а не по запросу на событие. `cancel` — снять
 *  отложенный повтор (размонтирование). */
export function throttle(fn: () => void, ms: number): (() => void) & { cancel: () => void } {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pending = false;
  const fire = () => {
    timer = setTimeout(() => {
      timer = null;
      if (pending) {
        pending = false;
        fn();
        fire();
      }
    }, ms);
  };
  const call = () => {
    if (timer) {
      pending = true;
      return;
    }
    fn();
    fire();
  };
  call.cancel = () => {
    if (timer) clearTimeout(timer);
    timer = null;
    pending = false;
  };
  return call;
}
