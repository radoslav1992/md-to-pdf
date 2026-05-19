import { useCallback, useEffect, useState } from 'react';
import { api, type PublicUser, type Role } from '../lib/api';

const ROLES: Role[] = ['free', 'premium', 'admin'];

export default function AdminUserList() {
  const [users, setUsers] = useState<PublicUser[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<number | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      const res = await api.listUsers();
      setUsers(res.items);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const onChangeRole = useCallback(async (id: number, role: Role) => {
    setBusyId(id);
    setError(null);
    try {
      const res = await api.updateUserRole(id, role);
      setUsers((prev) => prev?.map((u) => (u.id === res.user.id ? res.user : u)) ?? null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusyId(null);
    }
  }, []);

  if (users === null && !error) {
    return <p className="text-stone-500 text-sm">Loading…</p>;
  }

  return (
    <div>
      {error && (
        <div className="text-sm text-danger-700 bg-danger-50 border border-danger-100 rounded px-3 py-2 mb-3">
          {error}
        </div>
      )}
      <div className="overflow-x-auto border border-stone-200 rounded-lg bg-white">
        <table className="w-full text-sm">
          <thead className="bg-stone-50 text-left text-xs uppercase tracking-wide text-stone-500">
            <tr>
              <th className="px-3 py-2">ID</th>
              <th className="px-3 py-2">Email</th>
              <th className="px-3 py-2">Role</th>
              <th className="px-3 py-2">Created</th>
              <th className="px-3 py-2 text-right">Actions</th>
            </tr>
          </thead>
          <tbody>
            {users?.map((user) => (
              <tr key={user.id} className="border-t border-stone-100">
                <td className="px-3 py-2 text-stone-500">{user.id}</td>
                <td className="px-3 py-2 font-medium text-stone-900">{user.email}</td>
                <td className="px-3 py-2">
                  <RoleBadge role={user.role} />
                </td>
                <td className="px-3 py-2 text-stone-500 text-xs">
                  {new Date(user.created_at * 1000).toLocaleDateString()}
                </td>
                <td className="px-3 py-2 text-right">
                  <select
                    value={user.role}
                    disabled={busyId === user.id}
                    onChange={(e) => onChangeRole(user.id, e.target.value as Role)}
                    className="border border-stone-300 rounded-md px-2 py-1 text-xs bg-white"
                  >
                    {ROLES.map((r) => (
                      <option key={r} value={r}>
                        {r}
                      </option>
                    ))}
                  </select>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function RoleBadge({ role }: { role: Role }) {
  const classes =
    role === 'admin'
      ? 'bg-purple-100 text-purple-800'
      : role === 'premium'
      ? 'bg-warn-100 text-warn-800'
      : 'bg-stone-100 text-stone-700';
  return (
    <span
      className={`text-[10px] uppercase tracking-wide font-semibold px-2 py-0.5 rounded ${classes}`}
    >
      {role}
    </span>
  );
}
