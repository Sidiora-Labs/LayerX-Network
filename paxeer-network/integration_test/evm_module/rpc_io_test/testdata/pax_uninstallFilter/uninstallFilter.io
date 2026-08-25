// Pax namespace: uninstall filter
>> {"jsonrpc":"2.0","id":1,"method":"pax_newBlockFilter"}
<< {"jsonrpc":"2.0","id":1,"result":"0x1"}
>> {"jsonrpc":"2.0","id":2,"method":"pax_uninstallFilter","params":["0x1"]}
<< {"jsonrpc":"2.0","id":2,"result":true}
