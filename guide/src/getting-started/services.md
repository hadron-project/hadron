Services
========
Services allow users to model streaming-first microservice architectures within Railgun.

- A service is a set of RPC endpoints grouped together within a namespace, each endpoint having an associated pipeline.
- The service API is built upon the primitive components of RPC messaging, streaming & pipelines. The service API will ensure that all resources are in place and ready for use.
- This powerful abstraction allows teams to build simple and stateless gateways — GraphQL, REST &c — which communicate with peer backend microservices via the Railgun service abstraction.
- A response may be returned from any stage of a service endpoint's pipeline. Other stages of the pipeline may execute asynchronous work even after the response is returned.
- Services free teams from having to build their own networking stacks for service-to-service communication by leveraging Railgun.
- Services build upon the pipelines feature which enables teams to build streaming-first systems with minimal overhead, less error handling and with exactly-once semantics enforced on the server as needed.

Think of a service as a traditional microservice, where each endpoint of the service is like a traditional RPC or REST endpoint, and the associated pipeline is the function which is applied to the received payload, along with any other arbitrarily complex workflows which need to take place in the pipeline.
