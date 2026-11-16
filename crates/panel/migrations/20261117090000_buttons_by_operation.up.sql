-- A button names the operation it calls and that path's parameters; the conductor resolves the rest
-- in its catalog. Every button sent so far is one of sites' two, on `/api/v1/leads/{brand}/{lead}/…`.
UPDATE telegram_outbox SET buttons = (
	SELECT json_group_array(json_set(b.value, '$.call', json_object(
		'operation', CASE WHEN b.value ->> '$.call.path' LIKE '%/stage' THEN 'move_lead' ELSE 'no_answer' END,
		'path', json_object(
			'brand', substr(r, 1, instr(r, '/') - 1),
			'lead', substr(substr(r, instr(r, '/') + 1), 1, instr(substr(r, instr(r, '/') + 1), '/') - 1)
		),
		'body', json(b.value -> '$.call.body')
	)))
	FROM (SELECT value, substr(value ->> '$.call.path', length('/api/v1/leads/') + 1) AS r FROM json_each(telegram_outbox.buttons)) b
) WHERE json_array_length(buttons) > 0;
