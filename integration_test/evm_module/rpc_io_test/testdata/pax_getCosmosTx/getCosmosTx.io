// Pax: get Cosmos tx hash for EVM tx hash; non-existent tx returns error
>> {"jsonrpc":"2.0","id":1,"method":"pax_getCosmosTx","params":["0x0000000000000000000000000000000000000000000000000000000000000000"]}
<< {"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"receipt not found"}}
