def payload: .fields.payload | fromjson;
def mine: select(.target | startswith("hobgoblin"));
def brief: "\(.level) \(.target) \(.fields.event // .fields.message)";
def req: mine | select(.fields.event == "llm_request") | payload;
def resp: mine | select(.fields.event == "llm_response") | payload;
