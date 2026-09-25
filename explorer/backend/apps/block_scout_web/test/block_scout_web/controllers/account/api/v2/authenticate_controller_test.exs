defmodule BlockScoutWeb.Account.API.V2.AuthenticateControllerTest do
  use BlockScoutWeb.ConnCase, async: false

  alias Explorer.Account.Identity
  alias Explorer.Chain.Address
  alias Explorer.ThirdPartyIntegrations.Dynamic
  alias Explorer.ThirdPartyIntegrations.Dynamic.Strategy

  import Mox

  describe "POST api/account/v2/send_otp" do
    test "send OTP successfully", %{conn: conn} do
      Tesla.Test.expect_tesla_call(
        times: 3,
        returns: fn
          %{
            method: :post,
            url: "https://example.com/oauth/token",
            query: [],
            headers: [{"Content-type", "application/json"}],
            body:
              ~s|{"audience":"https://example.com/api/v2/","client_id":"client_id","client_secret":"secrets","grant_type":"client_credentials"}|
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s({"access_token": "test_token", "expires_in": 86400})
             }}

          %Tesla.Env{
            method: :get,
            url: "https://example.com/api/v2/users",
            query: [q: ~s|email:"test@example.com" OR user_metadata.email:"test@example.com"|],
            headers: [{"accept", "application/json"}, {"authorization", "Bearer test_token"}],
            body: ""
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s|[]|
             }}

          %{
            method: :post,
            url: "https://example.com/passwordless/start",
            query: %{},
            headers: [
              {"accept", "application/json"},
              {"auth0-forwarded-for", _ip},
              {"content-type", "application/json"}
            ],
            body:
              ~s|{"send":"code","connection":"email","email":"test@example.com","client_id":"client_id","client_secret":"secrets"}|
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s|{"_id":"123","email":"test@example.com","email_verified":false}|
             }}
        end
      )

      response =
        conn
        |> put_req_header("content-type", "application/json")
        |> post("/api/account/v2/send_otp", JSON.encode!(%{"email" => "test@example.com"}))
        |> json_response(200)

      assert response == %{"message" => "Success"}
    end

    test "send OTP for linking email to an existing account successfully", %{conn: conn} do
      auth = :auth |> build() |> put_in([Access.key!(:info), Access.key!(:email)], nil)
      {:ok, user} = Identity.find_or_create(auth)
      conn_with_user = Plug.Test.init_test_session(conn, current_user: user)

      Tesla.Test.expect_tesla_call(
        times: 3,
        returns: fn
          %{
            method: :post,
            url: "https://example.com/oauth/token",
            query: [],
            headers: [{"Content-type", "application/json"}],
            body:
              ~s|{"audience":"https://example.com/api/v2/","client_id":"client_id","client_secret":"secrets","grant_type":"client_credentials"}|
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s({"access_token": "test_token", "expires_in": 86400})
             }}

          %Tesla.Env{
            method: :get,
            url: "https://example.com/api/v2/users",
            query: [q: ~s|email:"test@example.com" OR user_metadata.email:"test@example.com"|],
            headers: [{"accept", "application/json"}, {"authorization", "Bearer test_token"}],
            body: ""
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s|[]|
             }}

          %{
            method: :post,
            url: "https://example.com/passwordless/start",
            query: %{},
            headers: [
              {"accept", "application/json"},
              {"auth0-forwarded-for", _ip},
              {"content-type", "application/json"}
            ],
            body:
              ~s|{"send":"code","connection":"email","email":"test@example.com","client_id":"client_id","client_secret":"secrets"}|
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s|{"_id":"123","email":"test@example.com","email_verified":false}|
             }}
        end
      )

      response =
        conn_with_user
        |> put_req_header("content-type", "application/json")
        |> post("/api/account/v2/send_otp", JSON.encode!(%{"email" => "test@example.com"}))
        |> json_response(200)

      assert response == %{"message" => "Success"}
    end

    test "do not send OTP for linking email to an existing account when email is already linked", %{conn: conn} do
      auth = :auth |> build() |> put_in([Access.key!(:info), Access.key!(:email)], nil)
      {:ok, user} = Identity.find_or_create(auth)
      conn_with_user = Plug.Test.init_test_session(conn, current_user: user)

      Tesla.Test.expect_tesla_call(
        times: 3,
        returns: fn
          %{
            method: :post,
            url: "https://example.com/oauth/token",
            query: [],
            headers: [{"Content-type", "application/json"}],
            body:
              ~s|{"audience":"https://example.com/api/v2/","client_id":"client_id","client_secret":"secrets","grant_type":"client_credentials"}|
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s({"access_token": "test_token", "expires_in": 86400})
             }}

          %Tesla.Env{
            method: :get,
            url: "https://example.com/api/v2/users",
            query: [q: ~s|email:"test@example.com" OR user_metadata.email:"test@example.com"|],
            headers: [{"accept", "application/json"}, {"authorization", "Bearer test_token"}],
            body: ""
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body:
                 ~s([{"identities":[{"connection":"email","user_id":"123","provider":"email","isSocial":false}],"user_id":"email|123","email":"test@example.com"}])
             }}

          %{
            method: :post,
            url: "https://example.com/passwordless/start",
            query: %{},
            headers: [
              {"accept", "application/json"},
              {"auth0-forwarded-for", _ip},
              {"content-type", "application/json"}
            ],
            body:
              ~s|{"send":"code","connection":"email","email":"test@example.com","client_id":"client_id","client_secret":"secrets"}|
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s|{"_id":"123","email":"test@example.com","email_verified":false}|
             }}
        end
      )

      response =
        conn_with_user
        |> put_req_header("content-type", "application/json")
        |> post("/api/account/v2/send_otp", JSON.encode!(%{"email" => "test@example.com"}))
        |> json_response(500)

      assert response == %{"message" => "Account with this email already exists"}
    end

    test "do nothing for an account with an existing email", %{conn: conn} do
      auth = build(:auth)
      {:ok, user} = Identity.find_or_create(auth)
      conn_with_user = Plug.Test.init_test_session(conn, current_user: user)

      Tesla.Test.expect_tesla_call(
        times: 3,
        returns: fn
          %{
            method: :post,
            url: "https://example.com/oauth/token",
            query: [],
            headers: [{"Content-type", "application/json"}],
            body:
              ~s|{"audience":"https://example.com/api/v2/","client_id":"client_id","client_secret":"secrets","grant_type":"client_credentials"}|
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s({"access_token": "test_token", "expires_in": 86400})
             }}

          %Tesla.Env{
            method: :get,
            url: "https://example.com/api/v2/users",
            query: [q: ~s|email:"test@example.com" OR user_metadata.email:"test@example.com"|],
            headers: [{"accept", "application/json"}, {"authorization", "Bearer test_token"}],
            body: ""
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s|[]|
             }}

          %{
            method: :post,
            url: "https://example.com/passwordless/start",
            query: %{},
            headers: [
              {"accept", "application/json"},
              {"auth0-forwarded-for", _ip},
              {"content-type", "application/json"}
            ],
            body:
              ~s|{"send":"code","connection":"email","email":"test@example.com","client_id":"client_id","client_secret":"secrets"}|
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s|{"_id":"123","email":"test@example.com","email_verified":false}|
             }}
        end
      )

      response =
        conn_with_user
        |> put_req_header("content-type", "application/json")
        |> post("/api/account/v2/send_otp", JSON.encode!(%{"email" => "test@example.com"}))
        |> json_response(500)

      assert response == %{"message" => "This account already has an email"}
    end
  end

  describe "POST api/account/v2/confirm_otp" do
    setup do
      initial_config = Application.get_env(:ueberauth, Ueberauth.Strategy.Auth0.OAuth)

      Application.put_env(
        :ueberauth,
        Ueberauth.Strategy.Auth0.OAuth,
        Keyword.put(initial_config, :auth0_application_id, "test_app")
      )

      on_exit(fn ->
        Application.put_env(:ueberauth, Ueberauth.Strategy.Auth0.OAuth, initial_config)
      end)

      :ok
    end

    # Regression test: after OpenApiSpex integration, confirm_otp must read
    # email and otp from conn.body_params instead of the action's params argument.
    # See commit dbf589ae25.
    test "confirm OTP successfully", %{conn: conn} do
      id_token = build_test_jwt(%{"sub" => "email|123", "email" => "test@example.com"})

      user_json =
        JSON.encode!(%{
          "user_id" => "email|123",
          "email" => "test@example.com",
          "email_verified" => true,
          "name" => "Test User",
          "nickname" => "test",
          "picture" => "https://example.com/avatar.png",
          "user_metadata" => %{
            "test_app" => %{
              "user_id" => "email|123",
              "name" => "Test User",
              "nickname" => "test",
              "picture" => "https://example.com/avatar.png"
            }
          }
        })

      Tesla.Test.expect_tesla_call(
        times: 3,
        returns: fn
          # OTP confirmation via OAuth2.Client
          %{
            method: :post,
            url: "https://example.com/oauth/token",
            headers: [
              {"accept", "application/json"},
              {"auth0-forwarded-for", _ip},
              {"content-type", "application/json"}
            ]
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s({"access_token":"test_access","id_token":"#{id_token}","token_type":"Bearer"})
             }}

          # M2M JWT via HttpClient
          %{
            method: :post,
            url: "https://example.com/oauth/token",
            query: [],
            headers: [{"Content-type", "application/json"}]
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s({"access_token": "test_token", "expires_in": 86400})
             }}

          # Get user by ID via OAuth2.Client
          %Tesla.Env{
            method: :get,
            url: "https://example.com/api/v2/users/" <> _,
            headers: [{"accept", "application/json"}, {"authorization", "Bearer test_token"}],
            body: ""
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: user_json
             }}
        end
      )

      response =
        conn
        |> put_req_header("content-type", "application/json")
        |> post("/api/account/v2/confirm_otp", JSON.encode!(%{"email" => "test@example.com", "otp" => "123456"}))
        |> json_response(200)

      assert response["email"] == "test@example.com"
      assert response["name"] == "Test User"
    end

    test "return error for wrong verification code", %{conn: conn} do
      Tesla.Test.expect_tesla_call(
        times: 1,
        returns: fn
          %{
            method: :post,
            url: "https://example.com/oauth/token",
            headers: [
              {"accept", "application/json"},
              {"auth0-forwarded-for", _ip},
              {"content-type", "application/json"}
            ]
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 403,
               body: ~s({"error":"invalid_grant","error_description":"Wrong email or verification code."})
             }}
        end
      )

      response =
        conn
        |> put_req_header("content-type", "application/json")
        |> post("/api/account/v2/confirm_otp", JSON.encode!(%{"email" => "test@example.com", "otp" => "000000"}))
        |> json_response(500)

      assert response == %{"message" => "Wrong verification code."}
    end

    test "return error when max attempts reached", %{conn: conn} do
      Tesla.Test.expect_tesla_call(
        times: 1,
        returns: fn
          %{
            method: :post,
            url: "https://example.com/oauth/token",
            headers: [
              {"accept", "application/json"},
              {"auth0-forwarded-for", _ip},
              {"content-type", "application/json"}
            ]
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 403,
               body:
                 ~s({"error":"invalid_grant","error_description":"You've reached the maximum number of attempts. Please try to login again."})
             }}
        end
      )

      response =
        conn
        |> put_req_header("content-type", "application/json")
        |> post("/api/account/v2/confirm_otp", JSON.encode!(%{"email" => "test@example.com", "otp" => "000000"}))
        |> json_response(500)

      assert response == %{"message" => "Max attempts reached. Please resend code."}
    end
  end

  describe "GET api/account/v2/siwe_message" do
    test "get SIWE message successfully", %{conn: conn} do
      address = build(:address)

      response =
        conn
        |> get("/api/account/v2/siwe_message?address=#{address.hash}")
        |> json_response(200)

      assert String.contains?(response["siwe_message"], Address.checksum(address))
    end

    test "return error for an invalid address", %{conn: conn} do
      response =
        conn
        |> get("/api/account/v2/siwe_message?address=invalid_address")
        |> json_response(422)

      assert response == %{
               "errors" => [
                 %{
                   "title" => "Invalid value",
                   "source" => %{"pointer" => "/address"},
                   "detail" => "Invalid format. Expected ~r/^0x([A-Fa-f0-9]{40})$/"
                 }
               ]
             }
    end
  end

  describe "POST api/account/v2/authenticate_via_wallet" do
    test "authenticate via wallet successfully", %{conn: conn} do
      private_key = :crypto.strong_rand_bytes(32)
      {:ok, <<0x04, public_key_raw::binary-size(64)>>} = ExSecp256k1.create_public_key(private_key)

      <<_::binary-size(12), address_bytes::binary-size(20)>> =
        ExKeccak.hash_256(public_key_raw)

      address_string = Address.checksum(address_bytes)

      Tesla.Test.expect_tesla_call(
        times: 2,
        returns: fn
          %{
            method: :post,
            url: "https://example.com/oauth/token",
            query: [],
            headers: [{"Content-type", "application/json"}],
            body:
              ~s|{"audience":"https://example.com/api/v2/","client_id":"client_id","client_secret":"secrets","grant_type":"client_credentials"}|
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body: ~s({"access_token": "test_token", "expires_in": 86400})
             }}

          %Tesla.Env{
            method: :get,
            url: "https://example.com/api/v2/users",
            query: _,
            headers: [{"accept", "application/json"}, {"authorization", "Bearer test_token"}],
            body: ""
          },
          _opts ->
            {:ok,
             %Tesla.Env{
               status: 200,
               body:
                 ~s([{"identities":[{"connection":"email","user_id":"123","provider":"email","isSocial":false}],"user_id":"email|123","email":"test@example.com","user_metadata":{"web3_address_hash":"#{address_string}"}}])
             }}
        end
      )

      message =
        conn
        |> get("/api/account/v2/siwe_message?address=#{address_string}")
        |> json_response(200)
        |> Map.get("siwe_message")

      # cspell:disable-next-line
      hash = ExKeccak.hash_256("\x19Ethereum Signed Message:\n#{byte_size(message)}" <> message)

      {:ok, {rs_binary, v}} = ExSecp256k1.sign_compact(hash, private_key)
      signature = "0x" <> Base.encode16(rs_binary <> <<v + 27>>, case: :lower)

      response =
        conn
        |> put_req_header("content-type", "application/json")
        |> post(
          "/api/account/v2/authenticate_via_wallet",
          JSON.encode!(%{"message" => message, "signature" => signature})
        )
        |> json_response(200)

      assert response["email"] == "test@example.com" and response["address_hash"] == address_string
    end

    test "return error for invalid signature", %{conn: conn} do
      response =
        conn
        |> put_req_header("content-type", "application/json")
        |> post(
          "/api/account/v2/authenticate_via_wallet",
          JSON.encode!(%{"message" => "test_message", "signature" => "0x1234"})
        )
        |> json_response(500)

      assert %{"message" => _error_message} = response
    end
  end

  describe "GET api/account/v2/authenticate_via_dynamic" do
    setup :set_mox_global

    test "authenticate via dynamic successfully", %{conn: conn} do
      initial_dynamic_env = Application.get_env(:explorer, Dynamic)
      initial_strategy_env = Application.get_env(:explorer, Strategy)

      Application.put_env(
        :explorer,
        Dynamic,
        Keyword.merge(initial_dynamic_env,
          enabled: true,
          env_id: "test_env",
          url: "https://app.dynamic.xyz/api/v0/sdk/test_env/.well-known/jwks"
        )
      )

      Application.put_env(
        :explorer,
        Strategy,
        Keyword.merge(initial_strategy_env, enabled: true)
      )

      on_exit(fn ->
        Application.put_env(:explorer, Dynamic, initial_dynamic_env)
        Application.put_env(:explorer, Strategy, initial_strategy_env)
      end)

      signing_key = JOSE.JWK.generate_key({:rsa, 2048})
      key_id = "dynamic-test-signing-key"

      Tesla.Test.expect_tesla_call(
        times: 1,
        returns: fn %{url: "https://app.dynamic.xyz/api/v0/sdk/test_env/.well-known/jwks"}, _opts ->
          {:ok,
           %Tesla.Env{
             status: 200,
             headers: [{"content-type", "application/json"}],
             body: JSON.encode!(%{"keys" => [public_jwk(signing_key, key_id)]})
           }}
        end
      )

      start_supervised!(Strategy)

      :timer.sleep(500)

      response =
        conn
        |> put_req_header(
          "authorization",
          "Bearer " <> sign_dynamic_token(signing_key, key_id, dynamic_claims())
        )
        |> get("/api/account/v2/authenticate_via_dynamic")
        |> json_response(200)

      assert response["email"] == "test@example.com"
    end

    test "without bearer token returns error", %{conn: conn} do
      initial_dynamic_env = Application.get_env(:explorer, Dynamic)
      initial_strategy_env = Application.get_env(:explorer, Strategy)

      Application.put_env(
        :explorer,
        Dynamic,
        Keyword.merge(initial_dynamic_env,
          enabled: true,
          env_id: "test_env",
          url: "https://app.dynamic.xyz/api/v0/sdk/test_env/.well-known/jwks"
        )
      )

      Application.put_env(
        :explorer,
        Strategy,
        Keyword.merge(initial_strategy_env, enabled: true)
      )

      on_exit(fn ->
        Application.put_env(:explorer, Dynamic, initial_dynamic_env)
        Application.put_env(:explorer, Strategy, initial_strategy_env)
      end)

      response =
        conn
        |> get("/api/account/v2/authenticate_via_dynamic")
        |> json_response(401)

      assert response == %{"message" => "No Bearer token"}
    end

    test "without config returns error", %{conn: conn} do
      response =
        conn
        |> put_req_header(
          "authorization",
          "Bearer some_token"
        )
        |> get("/api/account/v2/authenticate_via_dynamic")
        |> json_response(404)

      assert response == %{"message" => "This endpoint is not configured"}
    end
  end

  defp build_test_jwt(claims) do
    header = Base.url_encode64(JSON.encode!(%{"alg" => "HS256", "typ" => "JWT"}), padding: false)
    payload = Base.url_encode64(JSON.encode!(claims), padding: false)
    signature = Base.url_encode64("test_signature", padding: false)
    "#{header}.#{payload}.#{signature}"
  end

  defp public_jwk(signing_key, key_id) do
    {_modules, public_key} =
      signing_key
      |> JOSE.JWK.to_public()
      |> JOSE.JWK.to_map()

    Map.merge(public_key, %{
      "alg" => "RS256",
      "ext" => true,
      "key_ops" => ["verify"],
      "kid" => key_id,
      "use" => "sig"
    })
  end

  defp sign_dynamic_token(signing_key, key_id, claims) do
    {_modules, token} =
      signing_key
      |> JOSE.JWT.sign(%{"alg" => "RS256", "kid" => key_id, "typ" => "JWT"}, claims)
      |> JOSE.JWS.compact()

    token
  end

  defp dynamic_claims do
    issued_at = System.system_time(:second)
    wallet_address = "0x03c363f48c4FE0F2Ec6efbD49F7b114b8A61c14b"

    %{
      "alias" => "alias",
      "aud" => "https://example.com",
      "email" => "test@example.com",
      "environment_id" => "test_env",
      "exp" => issued_at + 300,
      "family_name" => "ln",
      "given_name" => "fn",
      "iat" => issued_at,
      "iss" => "app.dynamicauth.com/test_env",
      "lists" => [],
      "metadata" => %{},
      "missing_fields" => [],
      "nbf" => issued_at,
      "new_user" => false,
      "sub" => "dynamic-test-user",
      "username" => "username",
      "verified_credentials" => [
        %{
          "address" => wallet_address,
          "chain" => "eip155",
          "format" => "blockchain",
          "id" => "dynamic-test-wallet-credential",
          "name_service" => %{},
          "public_identifier" => wallet_address,
          "signInEnabled" => true,
          "wallet_name" => "metamask",
          "wallet_provider" => "browserExtension"
        },
        %{
          "email" => "test@example.com",
          "format" => "email",
          "id" => "dynamic-test-email-credential",
          "public_identifier" => "test@example.com",
          "signInEnabled" => true
        }
      ]
    }
  end
end
