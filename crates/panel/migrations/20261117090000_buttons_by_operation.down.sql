UPDATE telegram_outbox SET buttons = (
	SELECT json_group_array(json_set(b.value, '$.call', json_object(
		'method', 'POST',
		'path', '/api/v1/leads/' || (b.value ->> '$.call.path.brand') || '/' || (b.value ->> '$.call.path.lead') || '/'
			|| CASE b.value ->> '$.call.operation' WHEN 'move_lead' THEN 'stage' ELSE 'no_answer' END,
		'body', json(b.value -> '$.call.body')
	)))
	FROM json_each(telegram_outbox.buttons) b
) WHERE json_array_length(buttons) > 0;
