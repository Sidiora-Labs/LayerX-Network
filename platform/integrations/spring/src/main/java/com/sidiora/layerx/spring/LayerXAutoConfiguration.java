package com.sidiora.layerx.spring;

import com.sidiora.layerx.sdk.verify.LocalVerifier;
import jakarta.servlet.Filter;
import java.io.IOException;
import java.nio.file.Path;
import org.springframework.boot.autoconfigure.AutoConfiguration;
import org.springframework.boot.autoconfigure.condition.ConditionalOnBean;
import org.springframework.boot.autoconfigure.condition.ConditionalOnClass;
import org.springframework.boot.autoconfigure.condition.ConditionalOnMissingBean;
import org.springframework.boot.context.properties.EnableConfigurationProperties;
import org.springframework.boot.web.servlet.FilterRegistrationBean;
import org.springframework.context.annotation.Bean;
import org.springframework.core.Ordered;
import org.springframework.core.env.Environment;

@AutoConfiguration
@ConditionalOnClass({Filter.class, LocalVerifier.class})
@EnableConfigurationProperties(LayerXProperties.class)
public class LayerXAutoConfiguration {
    @Bean
    @ConditionalOnMissingBean
    public LayerXDeclaredConfig layerXDeclaredConfig(LayerXProperties properties, Environment environment) {
        PublishedSecretGuard.assertNoPublishedSecrets(environment);
        return properties.toDeclaredConfig();
    }

    @Bean
    @ConditionalOnMissingBean
    public SellerMiddleware.AuthorizedBatchResolver layerXAuthorizedBatchResolver(LayerXDeclaredConfig config) {
        return SellerMiddleware.staticAuthorizedBatches(config.authorizedBatch());
    }

    @Bean
    @ConditionalOnMissingBean
    public SellerMiddleware.PaymentAuthority layerXPaymentAuthority(
            SellerMiddleware.AuthorizedBatchResolver authorizedBatches) {
        return new SellerMiddleware.ReceiptPayloadAuthority(authorizedBatches);
    }

    @Bean
    @ConditionalOnMissingBean
    public Fulfillments.FulfillmentRepository layerXFulfillmentRepository(
            LayerXProperties properties) throws IOException {
        return DurableStores.fulfillments(Path.of(requiredStorageDirectory(properties)));
    }

    @Bean
    @ConditionalOnMissingBean
    public Webhooks.DeliveryStore layerXWebhookDeliveryStore(LayerXProperties properties) throws IOException {
        return DurableStores.deliveries(Path.of(requiredStorageDirectory(properties)));
    }

    @Bean
    @ConditionalOnMissingBean
    public SellerMiddleware layerXSellerMiddleware(LayerXDeclaredConfig config,
                                                   SellerMiddleware.PaymentAuthority authority,
                                                   Fulfillments.FulfillmentRepository fulfillments) {
        return new SellerMiddleware(config.paymentRequired(), authority, fulfillments);
    }

    @Bean
    @ConditionalOnMissingBean
    public Webhooks.VerifiedWebhookConsumer layerXWebhookConsumer(LayerXDeclaredConfig config,
                                                                   Webhooks.DeliveryStore deliveries) {
        return new Webhooks.VerifiedWebhookConsumer(config.webhookPublicKeys(), deliveries,
            config.webhookMaximumAgeMs(), config.webhookLeaseMs(), null);
    }

    @Bean
    @ConditionalOnBean(LayerXResourceHandler.class)
    public FilterRegistrationBean<LayerXPaymentGateFilter> layerXPaymentGateRegistration(
            LayerXDeclaredConfig config, SellerMiddleware seller, LayerXResourceHandler resources) {
        FilterRegistrationBean<LayerXPaymentGateFilter> registration =
            new FilterRegistrationBean<>(new LayerXPaymentGateFilter(config, seller, resources));
        registration.addUrlPatterns(config.protectedPath(), urlPattern(config.protectedPath()));
        registration.setName("layerXPaymentGateFilter");
        registration.setOrder(Ordered.HIGHEST_PRECEDENCE + 100);
        return registration;
    }

    @Bean
    @ConditionalOnBean(LayerXWebhookEventHandler.class)
    public FilterRegistrationBean<LayerXWebhookFilter> layerXWebhookRegistration(
            LayerXDeclaredConfig config, Webhooks.VerifiedWebhookConsumer consumer,
            LayerXWebhookEventHandler events) {
        FilterRegistrationBean<LayerXWebhookFilter> registration =
            new FilterRegistrationBean<>(new LayerXWebhookFilter(config, consumer, events));
        registration.addUrlPatterns(config.webhookPath());
        registration.setName("layerXWebhookFilter");
        registration.setOrder(Ordered.HIGHEST_PRECEDENCE + 101);
        return registration;
    }

    static String urlPattern(String mount) {
        return "/".equals(mount) ? "/*" : mount + "/*";
    }

    private static String requiredStorageDirectory(LayerXProperties properties) {
        String value = properties.getStorageDirectory();
        if (value == null || value.isBlank()) {
            throw new IllegalStateException("layerx.storage-directory is required for durable replay state");
        }
        return value;
    }
}
