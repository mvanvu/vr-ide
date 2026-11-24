{{-- Lists the users. --}}
<!-- user list -->
<ul class="users">
@foreach ($users as $user)
    <li title="user">{{ $user->name }}</li>
@endforeach
</ul>
@if ($users->isEmpty())
    <p>No users</p>
@endif
