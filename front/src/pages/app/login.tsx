import { Button } from '@/components/ui/button';
import { Link } from '@/components/ui/link';
import me from '@/services/me';

// Экран входа: единственная точка входа в UI — через SSO. Показывается вместо
// приложения, когда ядро требует токен (SSO включён) и валидного токена нет.
const LoginPage = () => {
  return (
    <div className="flex h-screen flex-col items-center justify-center gap-4 bg-slate-50">
      <div className="text-3xl font-semibold text-slate-800">aga</div>
      <div className="text-slate-500">Доступ только для участников</div>
      <Link href={me.loginUrl}>
        <Button variant="primary">Войти через SSO</Button>
      </Link>
    </div>
  );
};

export default LoginPage;