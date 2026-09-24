# <h1 align="center"> Smart-contract Verifier (Logic) </h1>

Smart-contract verification service. Contains the main verification logic
and exposes interface through which verification function could be called. 
Should be wrapped into binary providing protocol implementations for communication.

Currently, Rest API over HTTP and GRPC server implementation is available and can be found at 
[smart-contract-verifier-server](../smart-contract-verifier-server)