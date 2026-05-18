import { useEffect, useState } from 'react';
import { api, type PublicUser } from './api';

export type UserState =
  | { status: 'loading' }
  | { status: 'anon' }
  | { status: 'authed'; user: PublicUser };

export function useUser(): UserState {
  const [state, setState] = useState<UserState>({ status: 'loading' });
  useEffect(() => {
    let cancelled = false;
    api
      .me()
      .then((res) => {
        if (cancelled) return;
        if (res.user) setState({ status: 'authed', user: res.user });
        else setState({ status: 'anon' });
      })
      .catch(() => {
        if (!cancelled) setState({ status: 'anon' });
      });
    return () => {
      cancelled = true;
    };
  }, []);
  return state;
}
