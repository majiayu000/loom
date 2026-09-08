-- Run in the Supabase Auth project's SQL editor, then enable this function
-- under Authentication > Hooks > Custom Access Token. Not an API migration.
-- Keep the Auth database separate from the Loom service database.
begin;

create or replace function public.loom_access_token_hook(event jsonb)
returns jsonb
language plpgsql
stable
security invoker
set search_path = ''
as $$
declare
  verified boolean;
begin
  select exists (
    select 1 from auth.users
    where id = (event ->> 'user_id')::uuid
      and email_confirmed_at is not null
      and email is not null
      and email <> ''
      and email = event -> 'claims' ->> 'email'
  ) into verified;
  return jsonb_set(event, '{claims,email_verified}', to_jsonb(verified), true);
end;
$$;

revoke all on function public.loom_access_token_hook(jsonb) from public, anon, authenticated;
grant usage on schema public to supabase_auth_admin;
grant execute on function public.loom_access_token_hook(jsonb) to supabase_auth_admin;
-- supabase_auth_admin already owns and can read auth.users in Supabase.

commit;
